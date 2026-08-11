use crate::migration;
use asset_core::CoreError;
use asset_core::domain::{
    Directory, DirectoryId, DirectoryKind, DirectoryPath, Resource, ResourceContent, ResourceId,
    ResourceKind,
};
use asset_core::port::{
    DirectoryLocation, DirectoryRepository, ListResources, LocatedResource, ResourcePage,
    ResourceQuery, ResourceRepository,
};
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::path::Path;

const RESOURCE_SELECT: &str = r#"
    WITH RECURSIVE directory_paths(id, parent_id, name, path) AS (
        SELECT id, parent_id, name, ''
        FROM directories
        WHERE id = '00000000-0000-0000-0000-000000000000'
        UNION ALL
        SELECT child.id, child.parent_id, child.name,
               CASE
                   WHEN parent.path = '' THEN child.name
                   ELSE parent.path || '/' || child.name
               END
        FROM directories child
        JOIN directory_paths parent ON child.parent_id = parent.id
    )
    SELECT
        resources.id,
        resources.name,
        resources.directory_id,
        directory_paths.path AS directory_path,
        resources.kind,
        resources.content_json,
        resources.created_at,
        resources.updated_at,
        resources.revision,
        resources.deleted_at
    FROM resources
    JOIN directory_paths ON directory_paths.id = resources.directory_id
"#;

const RESOURCE_AGGREGATE_SELECT: &str = r#"
    SELECT
        resources.id,
        resources.name,
        resources.directory_id,
        resources.kind,
        resources.content_json,
        resources.created_at,
        resources.updated_at,
        resources.revision,
        resources.deleted_at
    FROM resources
"#;

/// SQLite 资源记录；领域值解析和聚合校验在读取后显式执行。
#[derive(sqlx::FromRow)]
struct ResourceRow {
    id: String,
    name: String,
    directory_id: String,
    kind: String,
    content_json: Option<String>,
    created_at: String,
    updated_at: String,
    revision: i64,
    deleted_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct LocatedResourceRow {
    #[sqlx(flatten)]
    resource: ResourceRow,
    directory_path: String,
}

/// SQLite 目录记录，与 Core 的目录聚合保持解耦。
#[derive(sqlx::FromRow)]
struct DirectoryRow {
    id: String,
    parent_id: Option<String>,
    name: String,
    kind: String,
    created_at: String,
    updated_at: String,
    revision: i64,
}

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
    pub async fn connect(sqlite_path: &Path, max_connections: u32) -> Result<Self, CoreError> {
        if max_connections == 0 {
            return Err(CoreError::configuration(
                "database.sqlite.max_connections must be greater than 0",
            ));
        }

        if let Some(parent) = sqlite_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| CoreError::repository("sqlite.create_dir", error))?;
        }

        let options = SqliteConnectOptions::new()
            .filename(sqlite_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
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
    pub(crate) fn pool(&self) -> &SqlitePool {
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
                directory_id,
                kind,
                content_json,
                created_at,
                updated_at,
                revision,
                deleted_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                directory_id = excluded.directory_id,
                kind = excluded.kind,
                content_json = excluded.content_json,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                revision = excluded.revision,
                deleted_at = excluded.deleted_at
            "#,
        )
        .bind(resource.id().to_string())
        .bind(resource.name())
        .bind(resource.directory_id().to_string())
        .bind(resource.kind().as_str())
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(encode_revision(resource.revision())?)
        .bind(resource.deleted_at().map(encode_timestamp))
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("save", error))?;

        Ok(())
    }

    async fn save_if_unchanged(
        &self,
        resource: &Resource,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let content_json = resource
            .content()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| CoreError::repository("resource.encode_content", error))?;

        let result = sqlx::query(
            r#"
            UPDATE resources SET
                name = ?, directory_id = ?, kind = ?, content_json = ?,
                created_at = ?, updated_at = ?, revision = ?, deleted_at = ?
            WHERE id = ? AND revision = ?
            "#,
        )
        .bind(resource.name())
        .bind(resource.directory_id().to_string())
        .bind(resource.kind().as_str())
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(encode_revision(resource.revision())?)
        .bind(resource.deleted_at().map(encode_timestamp))
        .bind(resource.id().to_string())
        .bind(encode_revision(expected_revision)?)
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("save_if_unchanged", error))?;
        Ok(result.rows_affected() == 1)
    }

    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        let statement = format!("{RESOURCE_AGGREGATE_SELECT} WHERE resources.id = ?");
        let row = sqlx::query_as::<_, ResourceRow>(&statement)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("find_by_id", error))?;

        row.map(decode_resource).transpose()
    }

    async fn remove_if_unchanged(
        &self,
        id: &ResourceId,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let result = sqlx::query("DELETE FROM resources WHERE id = ? AND revision = ?")
            .bind(id.to_string())
            .bind(encode_revision(expected_revision)?)
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
}

#[async_trait::async_trait]
impl ResourceQuery for SqliteResourceRepository {
    async fn find_located_by_id(
        &self,
        id: &ResourceId,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let statement = format!("{RESOURCE_SELECT} WHERE resources.id = ?");
        let row = sqlx::query_as::<_, LocatedResourceRow>(&statement)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("query.find_by_id", error))?;

        row.map(decode_located_resource).transpose()
    }

    async fn find_by_path(
        &self,
        directory: &DirectoryPath,
        name: &str,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let statement = format!(
            "{RESOURCE_SELECT} WHERE directory_paths.path = ? AND resources.name = ? \
             AND resources.deleted_at IS NULL"
        );
        let row = sqlx::query_as::<_, LocatedResourceRow>(&statement)
            .bind(directory.path())
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("find_by_path", error))?;

        row.map(decode_located_resource).transpose()
    }

    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError> {
        let total: i64 = build_list_count_query(query)
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(|error| CoreError::repository("list.count", error))?;
        let rows = build_list_select_query(query)
            .build_query_as::<LocatedResourceRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| CoreError::repository("list.select", error))?;
        let items = rows
            .into_iter()
            .map(decode_located_resource)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ResourcePage {
            items,
            total: total as u64,
            limit: query.limit(),
            offset: query.offset(),
        })
    }
}

#[async_trait::async_trait]
impl DirectoryRepository for SqliteResourceRepository {
    async fn load_all(&self) -> Result<Vec<Directory>, CoreError> {
        let rows = sqlx::query_as::<_, DirectoryRow>(
            "SELECT id, parent_id, name, kind, created_at, updated_at, revision FROM directories",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.load_all", error))?;
        rows.into_iter().map(decode_directory).collect()
    }

    async fn insert(&self, directory: &Directory) -> Result<(), CoreError> {
        sqlx::query(
            r#"
            INSERT INTO directories (
                id, parent_id, name, kind, created_at, updated_at, revision
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(directory.id().to_string())
        .bind(directory.parent_id().map(|id| id.to_string()))
        .bind(directory.name())
        .bind(directory.kind().as_str())
        .bind(encode_timestamp(directory.created_at()))
        .bind(encode_timestamp(directory.updated_at()))
        .bind(encode_directory_revision(directory.revision())?)
        .execute(&self.pool)
        .await
        .map_err(map_directory_write_error("directory.insert"))?;
        Ok(())
    }

    async fn save_if_unchanged(
        &self,
        directory: &Directory,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("directory.save.begin", error))?;
        if let Some(parent_id) = directory.parent_id() {
            let creates_cycle = sqlx::query_scalar::<_, i64>(
                r#"
                WITH RECURSIVE subtree(id) AS (
                    SELECT id FROM directories WHERE id = ?
                    UNION ALL
                    SELECT child.id
                    FROM directories child
                    JOIN subtree parent ON child.parent_id = parent.id
                )
                SELECT EXISTS(SELECT 1 FROM subtree WHERE id = ?)
                "#,
            )
            .bind(directory.id().to_string())
            .bind(parent_id.to_string())
            .fetch_one(&mut *transaction)
            .await
            .map_err(|error| CoreError::repository("directory.save.check_cycle", error))?;
            if creates_cycle != 0 {
                transaction
                    .rollback()
                    .await
                    .map_err(|error| CoreError::repository("directory.save.rollback", error))?;
                return Err(CoreError::conflict(
                    "moving the directory would create a cycle",
                ));
            }
        }
        let result = sqlx::query(
            r#"
            UPDATE directories
            SET parent_id = ?, name = ?, kind = ?, updated_at = ?, revision = ?
            WHERE id = ? AND revision = ?
            "#,
        )
        .bind(directory.parent_id().map(|id| id.to_string()))
        .bind(directory.name())
        .bind(directory.kind().as_str())
        .bind(encode_timestamp(directory.updated_at()))
        .bind(encode_directory_revision(directory.revision())?)
        .bind(directory.id().to_string())
        .bind(encode_directory_revision(expected_revision)?)
        .execute(&mut *transaction)
        .await
        .map_err(map_directory_write_error("directory.save_if_unchanged"))?;
        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("directory.save.commit", error))?;
        Ok(result.rows_affected() == 1)
    }

    async fn remove_if_empty(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        if id.is_root() {
            return Ok(false);
        }
        let result = sqlx::query(
            r#"
            DELETE FROM directories
            WHERE id = ? AND revision = ?
              AND NOT EXISTS (SELECT 1 FROM directories child WHERE child.parent_id = directories.id)
              AND NOT EXISTS (SELECT 1 FROM resources WHERE resources.directory_id = directories.id)
            "#,
        )
        .bind(id.to_string())
        .bind(encode_directory_revision(expected_revision)?)
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.remove_if_empty", error))?;
        Ok(result.rows_affected() == 1)
    }
}

fn map_directory_write_error(operation: &'static str) -> impl FnOnce(sqlx::Error) -> CoreError {
    move |error| {
        if error.to_string().contains("UNIQUE") {
            CoreError::conflict("a directory with the same name already exists")
        } else {
            CoreError::repository(operation, error)
        }
    }
}

fn build_list_count_query<'a>(query: &'a ListResources) -> QueryBuilder<'a, Sqlite> {
    let mut builder = QueryBuilder::new("SELECT COUNT(*) FROM resources");
    push_list_where(&mut builder, query);
    builder
}

fn build_list_select_query<'a>(query: &'a ListResources) -> QueryBuilder<'a, Sqlite> {
    let mut builder = QueryBuilder::new(RESOURCE_SELECT);
    push_list_where(&mut builder, query);
    builder.push(" ORDER BY resources.updated_at DESC, resources.id DESC LIMIT ");
    builder.push_bind(i64::from(query.limit()));
    builder.push(" OFFSET ");
    builder.push_bind(query.offset() as i64);
    builder
}

fn push_list_where<'a>(builder: &mut QueryBuilder<'a, Sqlite>, query: &'a ListResources) {
    let mut has_where = false;

    if !query.include_deleted() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.deleted_at IS NULL");
    }

    if !query.kinds().is_empty() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.kind IN (");
        let mut separated = builder.separated(", ");
        for kind in query.kinds() {
            separated.push_bind(kind.as_str());
        }
        separated.push_unseparated(")");
    }

    if let Some(q) = query.q() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.name LIKE ");
        builder.push_bind(format!("%{}%", escape_like(q)));
        builder.push(" ESCAPE '\\'");
    }

    if let Some(directory_id) = query.directory_id() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.directory_id = ");
        builder.push_bind(directory_id.to_string());
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

fn decode_resource(row: ResourceRow) -> Result<Resource, CoreError> {
    let ResourceRow {
        id,
        name,
        directory_id,
        kind,
        content_json,
        created_at,
        updated_at,
        revision,
        deleted_at,
    } = row;
    let kind = ResourceKind::try_new(kind)
        .map_err(|error| CoreError::repository("resource.decode_kind", error))?;
    let content = decode_content(content_json)?;
    let revision = u64::try_from(revision)
        .map_err(|error| CoreError::repository("resource.decode_revision", error))?;
    let deleted_at = deleted_at
        .map(|value| decode_timestamp("resource.decode_deleted_at", &value))
        .transpose()?;

    Resource::rehydrate(
        decode_id(&id)?,
        name,
        decode_directory_id(&directory_id)?,
        kind,
        content,
        decode_timestamp("resource.decode_created_at", &created_at)?,
        decode_timestamp("resource.decode_updated_at", &updated_at)?,
        revision,
        deleted_at,
    )
    .map_err(|error| CoreError::repository("resource.rehydrate", error))
}

fn decode_located_resource(row: LocatedResourceRow) -> Result<LocatedResource, CoreError> {
    let directory_id = decode_directory_id(&row.resource.directory_id)?;
    let directory_path = DirectoryPath::from_path(row.directory_path)
        .map_err(|error| CoreError::repository("resource.decode_directory_path", error))?;
    let resource = decode_resource(row.resource)?;
    LocatedResource::new(
        resource,
        DirectoryLocation::new(directory_id, directory_path),
    )
}

fn decode_id(value: &str) -> Result<ResourceId, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("resource.decode_id", error))
}

fn decode_directory_id(value: &str) -> Result<DirectoryId, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("directory.decode_id", error))
}

fn decode_directory(row: DirectoryRow) -> Result<Directory, CoreError> {
    Directory::rehydrate(
        decode_directory_id(&row.id)?,
        row.parent_id
            .as_deref()
            .map(decode_directory_id)
            .transpose()?,
        row.name,
        DirectoryKind::try_new(row.kind)
            .map_err(|error| CoreError::repository("directory.decode_kind", error))?,
        decode_timestamp("directory.decode_created_at", &row.created_at)?,
        decode_timestamp("directory.decode_updated_at", &row.updated_at)?,
        decode_revision(row.revision)?,
    )
    .map_err(|error| CoreError::repository("directory.rehydrate", error))
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

fn encode_revision(value: u64) -> Result<i64, CoreError> {
    i64::try_from(value).map_err(|error| CoreError::repository("resource.encode_revision", error))
}

fn decode_revision(value: i64) -> Result<u64, CoreError> {
    u64::try_from(value).map_err(|error| CoreError::repository("directory.decode_revision", error))
}

fn encode_directory_revision(value: u64) -> Result<i64, CoreError> {
    i64::try_from(value).map_err(|error| CoreError::repository("directory.encode_revision", error))
}

fn decode_timestamp(operation: &'static str, value: &str) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| CoreError::repository(operation, error))
}

#[cfg(test)]
mod tests;
