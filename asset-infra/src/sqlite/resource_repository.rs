use crate::{config::DatabaseConfig, migration};
use asset_core::CoreError;
use asset_core::domain::{
    Resource, ResourceContent, ResourceDirectory, ResourceId, ResourceKind, ResourceMetadata,
    ResourceSnapshot, ResourceStatus, StorageKey,
};
use asset_core::port::{ListResources, ResourcePage, ResourceQuery, ResourceRepository};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::fmt;

/// SQLite 版本的资源聚合仓储。
///
/// 该实现负责把 `Resource` 聚合保存到本地 SQLite 数据库。它是 `asset-core` 中
/// `ResourceRepository` 端口的基础设施适配器。
#[derive(Clone)]
pub struct SqliteResourceRepository {
    pool: SqlitePool,
}

impl SqliteResourceRepository {
    /// 连接 SQLite，并执行尚未应用的数据库迁移。
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, CoreError> {
        if config.max_connections == 0 {
            return Err(CoreError::configuration(
                "database.max_connections must be greater than 0",
            ));
        }

        if let Some(parent) = config.sqlite_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| CoreError::repository("sqlite.create_dir", error))?;
        }

        let options = SqliteConnectOptions::new()
            .filename(&config.sqlite_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(config.max_connections)
            .connect_with(options)
            .await
            .map_err(|error| CoreError::repository("sqlite.connect", error))?;

        let repository = Self { pool };
        repository.run_migrations().await?;

        Ok(repository)
    }

    /// 使用已有连接池创建仓储。
    ///
    /// 该构造函数不会自动执行迁移，主要用于测试或由外部迁移系统管理 schema 的场景。
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// 返回内部 SQLite 连接池。
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 执行 SQLite 数据库迁移。
    pub async fn run_migrations(&self) -> Result<(), CoreError> {
        migration::sqlite::run(&self.pool).await
    }
}

#[async_trait::async_trait]
impl ResourceRepository for SqliteResourceRepository {
    async fn health_check(&self) -> Result<(), CoreError> {
        sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::repository("health_check", error))
    }

    async fn save(&self, resource: &Resource) -> Result<(), CoreError> {
        ensure_directory_path(&self.pool, resource.directory().path()).await?;

        let metadata_json = serde_json::to_string(resource.metadata())
            .map_err(|error| CoreError::repository("resource.encode_metadata", error))?;
        let content_json = resource
            .content()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| CoreError::repository("resource.encode_content", error))?;

        sqlx::query(
            r#"
            INSERT INTO resources (
                id,
                name,
                directory,
                kind,
                status,
                metadata_json,
                content_json,
                created_at,
                updated_at,
                deleted_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                directory = excluded.directory,
                kind = excluded.kind,
                status = excluded.status,
                metadata_json = excluded.metadata_json,
                content_json = excluded.content_json,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                deleted_at = excluded.deleted_at
            "#,
        )
        .bind(resource.id().to_string())
        .bind(resource.name())
        .bind(resource.directory().path())
        .bind(resource.kind().as_str())
        .bind(status_to_str(resource.status()))
        .bind(metadata_json)
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(resource.deleted_at().map(encode_timestamp))
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("save", error))?;

        Ok(())
    }

    async fn save_if_unchanged(
        &self,
        resource: &Resource,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError> {
        ensure_directory_path(&self.pool, resource.directory().path()).await?;
        let metadata_json = serde_json::to_string(resource.metadata())
            .map_err(|error| CoreError::repository("resource.encode_metadata", error))?;
        let content_json = resource
            .content()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| CoreError::repository("resource.encode_content", error))?;

        let result = sqlx::query(
            r#"
            UPDATE resources SET
                name = ?, directory = ?, kind = ?, status = ?, metadata_json = ?,
                content_json = ?, created_at = ?, updated_at = ?, deleted_at = ?
            WHERE id = ? AND updated_at = ?
            "#,
        )
        .bind(resource.name())
        .bind(resource.directory().path())
        .bind(resource.kind().as_str())
        .bind(status_to_str(resource.status()))
        .bind(metadata_json)
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(resource.deleted_at().map(encode_timestamp))
        .bind(resource.id().to_string())
        .bind(encode_timestamp(expected_updated_at))
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("save_if_unchanged", error))?;

        Ok(result.rows_affected() == 1)
    }

    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                name,
                directory,
                kind,
                status,
                metadata_json,
                content_json,
                created_at,
                updated_at,
                deleted_at
            FROM resources
            WHERE id = ?
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoreError::repository("find_by_id", error))?;

        row.map(decode_resource).transpose()
    }

    async fn remove_if_unchanged(
        &self,
        id: &ResourceId,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError> {
        let result = sqlx::query("DELETE FROM resources WHERE id = ? AND updated_at = ?")
            .bind(id.to_string())
            .bind(encode_timestamp(expected_updated_at))
            .execute(&self.pool)
            .await
            .map_err(|error| CoreError::repository("remove_if_unchanged", error))?;

        Ok(result.rows_affected() == 1)
    }

    async fn remove(&self, id: &ResourceId) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM resources WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|error| CoreError::repository("remove", error))?;

        Ok(())
    }

    async fn save_directory(&self, directory: &ResourceDirectory) -> Result<(), CoreError> {
        ensure_directory_path(&self.pool, directory.parent_path()).await?;
        let now = encode_timestamp(Utc::now());
        sqlx::query("INSERT INTO directories (path, parent_path, name, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
            .bind(directory.path())
            .bind(directory.parent_path())
            .bind(directory.name())
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    CoreError::conflict(format!("directory `{}` already exists", directory.path()))
                } else {
                    CoreError::repository("directory.create", error)
                }
            })?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ResourceQuery for SqliteResourceRepository {
    async fn find_by_content_key(&self, key: &StorageKey) -> Result<Option<Resource>, CoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, name, directory, kind, status, metadata_json, content_json,
                created_at, updated_at, deleted_at
            FROM resources
            WHERE json_extract(content_json, '$.key') = ?
            "#,
        )
        .bind(key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoreError::repository("find_by_content_key", error))?;

        row.map(decode_resource).transpose()
    }

    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError> {
        let total: i64 = build_list_count_query(query)
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(|error| CoreError::repository("list.count", error))?;
        let rows = build_list_select_query(query)
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| CoreError::repository("list.select", error))?;
        let items = rows
            .into_iter()
            .map(decode_resource)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ResourcePage {
            items,
            total: total as u64,
            limit: query.limit(),
            offset: query.offset(),
        })
    }

    async fn list_directories(
        &self,
        parent: &ResourceDirectory,
    ) -> Result<Vec<ResourceDirectory>, CoreError> {
        let rows = sqlx::query(
            r#"
            SELECT path, parent_path, name
            FROM directories
            WHERE parent_path = ?
            ORDER BY name ASC
            "#,
        )
        .bind(parent.path())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("list_directories", error))?;

        rows.into_iter()
            .map(|row| {
                ResourceDirectory::rehydrate(
                    column(&row, "path")?,
                    column(&row, "parent_path")?,
                    column(&row, "name")?,
                )
                .map_err(CoreError::from)
            })
            .collect()
    }
}

fn build_list_count_query(query: &ListResources) -> QueryBuilder<Sqlite> {
    let mut builder = QueryBuilder::new("SELECT COUNT(*) FROM resources");
    push_list_where(&mut builder, query);
    builder
}

fn build_list_select_query(query: &ListResources) -> QueryBuilder<Sqlite> {
    let mut builder = QueryBuilder::new(
        r#"
        SELECT
            id,
            name,
            directory,
            kind,
            status,
            metadata_json,
            content_json,
            created_at,
            updated_at,
            deleted_at
        FROM resources
        "#,
    );
    push_list_where(&mut builder, query);
    builder.push(" ORDER BY updated_at DESC, id DESC LIMIT ");
    builder.push_bind(i64::from(query.limit()));
    builder.push(" OFFSET ");
    builder.push_bind(query.offset() as i64);
    builder
}

fn push_list_where(builder: &mut QueryBuilder<Sqlite>, query: &ListResources) {
    let mut has_where = false;

    if !query.include_deleted() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("deleted_at IS NULL");
    }

    if !query.kinds().is_empty() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("kind IN (");
        let mut separated = builder.separated(", ");
        for kind in query.kinds() {
            separated.push_bind(kind.as_str());
        }
        separated.push_unseparated(")");
    }

    if let Some(tag) = query.tag() {
        push_condition_prefix(builder, &mut has_where);
        builder.push(
            "EXISTS (SELECT 1 FROM json_each(resources.metadata_json, '$.summary.tags') WHERE value = ",
        );
        builder.push_bind(tag);
        builder.push(")");
    }

    if let Some(q) = query.q() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("name LIKE ");
        builder.push_bind(format!("%{}%", escape_like(q)));
        builder.push(" ESCAPE '\\'");
    }

    if let Some(directory) = query.directory() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("directory = ");
        builder.push_bind(directory.path());
    }
}

fn push_condition_prefix(builder: &mut QueryBuilder<Sqlite>, has_where: &mut bool) {
    if *has_where {
        builder.push(" AND ");
    } else {
        builder.push(" WHERE ");
        *has_where = true;
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn decode_resource(row: SqliteRow) -> Result<Resource, CoreError> {
    let id = decode_id(column(&row, "id")?)?;
    let name = column(&row, "name")?;
    let directory = ResourceDirectory::from_path(column::<String>(&row, "directory")?)?;
    let kind = ResourceKind::try_new(column::<String>(&row, "kind")?)?;
    let status = status_from_str(column(&row, "status")?)?;
    let metadata = decode_metadata(column(&row, "metadata_json")?)?;
    let content = decode_content(column(&row, "content_json")?)?;
    let created_at = decode_timestamp(column(&row, "created_at")?)?;
    let updated_at = decode_timestamp(column(&row, "updated_at")?)?;
    let deleted_at = column::<Option<String>>(&row, "deleted_at")?
        .map(decode_timestamp)
        .transpose()?;

    Resource::rehydrate(ResourceSnapshot {
        id,
        name,
        directory,
        kind,
        status,
        metadata,
        content,
        created_at,
        updated_at,
        deleted_at,
    })
    .map_err(CoreError::from)
}

async fn ensure_directory_path(pool: &SqlitePool, directory: &str) -> Result<(), CoreError> {
    if directory.is_empty() {
        return Ok(());
    }

    let now = encode_timestamp(Utc::now());
    let mut parent_path = String::new();
    for name in directory.split('/') {
        let path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{parent_path}/{name}")
        };

        sqlx::query(
            r#"
            INSERT INTO directories (path, parent_path, name, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(path) DO UPDATE SET
                parent_path = excluded.parent_path,
                name = excluded.name,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&path)
        .bind(&parent_path)
        .bind(name)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|error| CoreError::repository("directory.save", error))?;

        parent_path = path;
    }

    Ok(())
}

fn column<T>(row: &SqliteRow, name: &'static str) -> Result<T, CoreError>
where
    for<'row> T: sqlx::Decode<'row, Sqlite> + sqlx::Type<Sqlite>,
{
    row.try_get(name)
        .map_err(|error| CoreError::repository("resource.decode_row", error))
}

fn decode_id(value: String) -> Result<ResourceId, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("resource.decode_id", error))
}

fn decode_metadata(value: String) -> Result<ResourceMetadata, CoreError> {
    serde_json::from_str::<Value>(&value)
        .map_err(|error| CoreError::repository("resource.decode_metadata", error))
        .and_then(|value| ResourceMetadata::from_persisted_value(value).map_err(CoreError::from))
}

fn decode_content(value: Option<String>) -> Result<Option<ResourceContent>, CoreError> {
    value
        .map(|value| {
            serde_json::from_str::<ResourceContent>(&value)
                .map_err(|error| CoreError::repository("resource.decode_content", error))
        })
        .transpose()
}

fn encode_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

fn decode_timestamp(value: String) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| CoreError::repository("resource.decode_timestamp", error))
}

fn status_to_str(status: ResourceStatus) -> &'static str {
    match status {
        ResourceStatus::Active => "active",
        ResourceStatus::Archived => "archived",
    }
}

fn status_from_str(value: String) -> Result<ResourceStatus, CoreError> {
    match value.as_str() {
        "active" => Ok(ResourceStatus::Active),
        "archived" => Ok(ResourceStatus::Archived),
        other => Err(CoreError::repository(
            "resource.decode_status",
            DecodeError(format!("unknown resource status `{other}`")),
        )),
    }
}

#[derive(Debug)]
struct DecodeError(String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_core::domain::{Checksum, ResourceContent, StorageKey};
    use std::path::PathBuf;

    #[tokio::test]
    async fn sqlite_repository_roundtrips_resource() {
        let repository = repository("roundtrip").await;
        let checksum = Checksum::sha256("a".repeat(64)).unwrap();
        let content = ResourceContent::builder(StorageKey::new("assets/image.png").unwrap(), 42)
            .with_mime_type("image/png")
            .with_original_filename("image.png")
            .with_checksum(checksum.clone())
            .build()
            .unwrap();
        let resource = Resource::builder("image")
            .with_kind("core:image")
            .with_metadata(
                ResourceMetadata::builder()
                    .with_tags(["rust", "asset"])
                    .build()
                    .unwrap(),
            )
            .with_content(content)
            .build()
            .unwrap();

        repository.save(&resource).await.unwrap();

        let restored = repository
            .find_by_id(&resource.id())
            .await
            .unwrap()
            .unwrap();
        let restored_content = restored.content().unwrap();

        assert_eq!(restored.id(), resource.id());
        assert_eq!(restored.name(), "image");
        assert!(restored.kind().is("core:image"));
        assert_eq!(restored.metadata().tags(), &["rust", "asset"]);
        assert_eq!(restored_content.key().as_str(), "assets/image.png");
        assert_eq!(restored_content.size(), 42);
        assert_eq!(restored_content.mime_type(), Some("image/png"));
        assert_eq!(restored_content.original_filename(), Some("image.png"));
        assert_eq!(
            restored_content.checksums().collect::<Vec<_>>(),
            vec![&checksum]
        );
    }

    #[tokio::test]
    async fn sqlite_repository_upserts_and_removes_resource() {
        let repository = repository("upsert-remove").await;
        let mut resource = Resource::builder("image")
            .with_kind("core:image")
            .build()
            .unwrap();

        repository.save(&resource).await.unwrap();
        resource.rename("renamed image").unwrap();
        repository.save(&resource).await.unwrap();

        let restored = repository
            .find_by_id(&resource.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.name(), "renamed image");

        repository.remove(&resource.id()).await.unwrap();
        repository.remove(&resource.id()).await.unwrap();

        assert!(
            repository
                .find_by_id(&resource.id())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn conditional_save_rejects_a_stale_resource_snapshot() {
        let repository = repository("conditional-save").await;
        let resource = Resource::builder("original").build().unwrap();
        repository.save(&resource).await.unwrap();

        let expected = resource.updated_at();
        let mut concurrent = resource.clone();
        concurrent.rename("concurrent").unwrap();
        repository.save(&concurrent).await.unwrap();

        let mut stale = resource.clone();
        stale.rename("stale").unwrap();
        assert!(
            !repository
                .save_if_unchanged(&stale, expected)
                .await
                .unwrap()
        );
        assert_eq!(
            repository
                .find_by_id(&resource.id())
                .await
                .unwrap()
                .unwrap()
                .name(),
            "concurrent"
        );
    }

    #[tokio::test]
    async fn conditional_remove_rejects_a_stale_resource_snapshot() {
        let repository = repository("conditional-remove").await;
        let resource = Resource::builder("original").build().unwrap();
        repository.save(&resource).await.unwrap();

        let expected = resource.updated_at();
        let mut concurrent = resource.clone();
        concurrent.rename("concurrent").unwrap();
        repository.save(&concurrent).await.unwrap();

        assert!(
            !repository
                .remove_if_unchanged(&resource.id(), expected)
                .await
                .unwrap()
        );
        assert!(
            repository
                .remove_if_unchanged(&resource.id(), concurrent.updated_at())
                .await
                .unwrap()
        );
        assert!(
            repository
                .find_by_id(&resource.id())
                .await
                .unwrap()
                .is_none()
        );
    }

    async fn repository(name: &str) -> SqliteResourceRepository {
        SqliteResourceRepository::connect(&DatabaseConfig {
            sqlite_path: unique_temp_path(name).join("asset-hub.sqlite"),
            max_connections: 1,
        })
        .await
        .unwrap()
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
    }
}
