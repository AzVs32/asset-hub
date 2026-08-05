use asset_core::CoreError;
use asset_core::domain::{
    ResourceContent, ResourceContentReplacement, ResourceContentReplacementId, ResourceId,
    StorageKey,
};
use asset_core::port::ResourceContentReplacementRepository;
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

#[derive(Clone)]
pub struct SqliteResourceContentReplacementRepository {
    pool: SqlitePool,
}

impl SqliteResourceContentReplacementRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ResourceContentReplacementRepository for SqliteResourceContentReplacementRepository {
    async fn save(&self, replacement: &ResourceContentReplacement) -> Result<(), CoreError> {
        let content = serde_json::to_string(replacement.replacement_content())
            .map_err(|error| CoreError::repository("content_replacement.encode_content", error))?;
        sqlx::query(
            r#"
            INSERT INTO resource_content_replacements (
                id, resource_id, expected_revision, target_key, staged_key, backup_key,
                replacement_content_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(replacement.id().to_string())
        .bind(replacement.resource_id().to_string())
        .bind(encode_revision(replacement.expected_revision())?)
        .bind(replacement.target_key().as_str())
        .bind(replacement.staged_key().as_str())
        .bind(replacement.backup_key().as_str())
        .bind(content)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|error| CoreError::repository("content_replacement.save", error))
    }

    async fn list_pending(&self) -> Result<Vec<ResourceContentReplacement>, CoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, resource_id, expected_revision, target_key, staged_key, backup_key,
                   replacement_content_json
            FROM resource_content_replacements
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("content_replacement.list", error))?;
        rows.into_iter().map(decode_replacement).collect()
    }

    async fn remove(&self, id: &ResourceContentReplacementId) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM resource_content_replacements WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::repository("content_replacement.remove", error))
    }
}

fn decode_replacement(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ResourceContentReplacement, CoreError> {
    let id = ResourceContentReplacementId::from_str(row.get::<String, _>("id").as_str())
        .map_err(|error| CoreError::repository("content_replacement.id", error))?;
    let resource_id = ResourceId::from_str(row.get::<String, _>("resource_id").as_str())
        .map_err(|error| CoreError::repository("content_replacement.resource_id", error))?;
    let expected_revision = u64::try_from(row.get::<i64, _>("expected_revision"))
        .map_err(|error| CoreError::repository("content_replacement.expected_revision", error))?;
    let content = serde_json::from_str::<ResourceContent>(
        row.get::<String, _>("replacement_content_json").as_str(),
    )
    .map_err(|error| CoreError::repository("content_replacement.decode_content", error))?;
    Ok(ResourceContentReplacement::rehydrate(
        id,
        resource_id,
        expected_revision,
        StorageKey::new(row.get::<String, _>("target_key"))?,
        StorageKey::new(row.get::<String, _>("staged_key"))?,
        StorageKey::new(row.get::<String, _>("backup_key"))?,
        content,
    )?)
}

fn encode_revision(value: u64) -> Result<i64, CoreError> {
    i64::try_from(value)
        .map_err(|_| CoreError::configuration("resource revision exceeds SQLite INTEGER range"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteResourceRepository;
    use asset_core::domain::{Checksum, Resource};
    use asset_core::port::ResourceRepository;

    #[tokio::test]
    async fn pending_replacement_roundtrips_and_is_unique_per_resource() {
        let path = std::env::temp_dir()
            .join(format!(
                "asset-hub-content-replacement-{}",
                uuid::Uuid::now_v7()
            ))
            .join("asset-hub.sqlite");
        let resources = SqliteResourceRepository::connect(&path, 1).await.unwrap();
        let repository = SqliteResourceContentReplacementRepository::new(resources.pool().clone());
        let content = ResourceContent::verified(3, Checksum::sha256("a".repeat(64)).unwrap())
            .with_mime_type("text/plain")
            .build()
            .unwrap();
        let resource = Resource::builder("note.txt")
            .with_content(content.clone())
            .build()
            .unwrap();
        resources.save(&resource).await.unwrap();
        let pending = ResourceContentReplacement::new(
            resource.id(),
            resource.revision(),
            StorageKey::new("note.txt").unwrap(),
            StorageKey::new(".asset-hub/uploads/replacement-test").unwrap(),
            StorageKey::new(".asset-hub/content-backups/replacement-test").unwrap(),
            content,
        )
        .unwrap();

        repository.save(&pending).await.unwrap();
        assert!(repository.save(&pending).await.is_err());
        assert_eq!(
            repository.list_pending().await.unwrap(),
            vec![pending.clone()]
        );

        repository.remove(&pending.id()).await.unwrap();
        repository.remove(&pending.id()).await.unwrap();
        assert!(repository.list_pending().await.unwrap().is_empty());
    }
}
