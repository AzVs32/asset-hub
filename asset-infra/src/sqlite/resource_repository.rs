use crate::{config::DatabaseConfig, migration};
use asset_core::CoreError;
use asset_core::domain::{
    Resource, ResourceContent, ResourceId, ResourceKind, ResourceMetadata, ResourceSnapshot,
    ResourceStatus,
};
use asset_core::port::ResourceRepository;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, Sqlite, SqlitePool};
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
    async fn save(&self, resource: &Resource) -> Result<(), CoreError> {
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
                kind,
                status,
                metadata_json,
                content_json,
                created_at,
                updated_at,
                deleted_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
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

    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                name,
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

    async fn remove(&self, id: &ResourceId) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM resources WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|error| CoreError::repository("remove", error))?;

        Ok(())
    }
}

fn decode_resource(row: SqliteRow) -> Result<Resource, CoreError> {
    let id = decode_id(column(&row, "id")?)?;
    let name = column(&row, "name")?;
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
    use serde_json::json;
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
            .with_kind("asset:image")
            .with_metadata(
                ResourceMetadata::builder()
                    .with_tags(["rust", "asset"])
                    .with_attribute("source", json!("sqlite-test"))
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
        assert!(restored.kind().is("asset:image"));
        assert_eq!(restored.metadata().tags(), &["rust", "asset"]);
        assert_eq!(
            restored.metadata().attribute("source"),
            Some(&json!("sqlite-test"))
        );
        assert_eq!(restored_content.key().as_str(), "assets/image.png");
        assert_eq!(restored_content.size(), 42);
        assert_eq!(restored_content.mime_type(), Some("image/png"));
        assert_eq!(restored_content.original_filename(), Some("image.png"));
        assert_eq!(restored_content.checksums(), &[checksum]);
    }

    #[tokio::test]
    async fn sqlite_repository_upserts_and_removes_resource() {
        let repository = repository("upsert-remove").await;
        let mut resource = Resource::builder("image")
            .with_kind("asset:image")
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
