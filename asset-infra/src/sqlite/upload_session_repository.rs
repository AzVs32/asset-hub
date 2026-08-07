use asset_core::CoreError;
use asset_core::domain::{
    Checksum, DirectoryPath, ResourceId, ResourceKind, UploadId, UploadSession,
    UploadSessionSnapshot, UploadStatus, UserId,
};
use asset_core::port::UploadSessionRepository;
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

#[derive(Clone)]
pub struct SqliteUploadSessionRepository {
    pool: SqlitePool,
}

impl SqliteUploadSessionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl UploadSessionRepository for SqliteUploadSessionRepository {
    async fn save(&self, session: &UploadSession) -> Result<(), CoreError> {
        sqlx::query(
            r#"
            INSERT INTO upload_sessions (
                id, resource_id, owner_id, name, directory, kind, mime_type,
                expected_size, offset, status, expected_checksum_value, actual_checksum_value,
                failure, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(session.id().to_string())
        .bind(session.resource_id().to_string())
        .bind(session.owner_id().to_string())
        .bind(session.name())
        .bind(session.directory().path())
        .bind(session.kind().as_str())
        .bind(session.mime_type())
        .bind(encode_u64(session.expected_size())?)
        .bind(encode_u64(session.offset())?)
        .bind(session.status().as_str())
        .bind(session.expected_checksum().value())
        .bind(session.actual_checksum().map(Checksum::value))
        .bind(session.failure())
        .bind(session.created_at().to_rfc3339())
        .bind(session.updated_at().to_rfc3339())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|error| CoreError::repository("upload_session.save", error))
    }

    async fn find_by_id(&self, id: &UploadId) -> Result<Option<UploadSession>, CoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, resource_id, owner_id, name, directory, kind, mime_type,
                   expected_size, offset, status, expected_checksum_value, actual_checksum_value,
                   failure, created_at, updated_at
            FROM upload_sessions
            WHERE id = ?
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoreError::repository("upload_session.find", error))?;
        row.map(decode_session).transpose()
    }

    async fn update_offset(
        &self,
        id: &UploadId,
        expected_offset: u64,
        offset: u64,
    ) -> Result<bool, CoreError> {
        let result = sqlx::query(
            r#"
            UPDATE upload_sessions
            SET offset = ?, updated_at = ?
            WHERE id = ? AND offset = ? AND status = 'uploading' AND ? <= expected_size
            "#,
        )
        .bind(encode_u64(offset)?)
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .bind(encode_u64(expected_offset)?)
        .bind(encode_u64(offset)?)
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("upload_session.update_offset", error))?;
        Ok(result.rows_affected() == 1)
    }

    async fn remove(&self, id: &UploadId) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM upload_sessions WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::repository("upload_session.remove", error))
    }

    async fn mark_finalizing(&self, id: &UploadId) -> Result<bool, CoreError> {
        let result = sqlx::query(
            r#"
            UPDATE upload_sessions
            SET status = 'finalizing', failure = NULL, updated_at = ?
            WHERE id = ?
              AND status IN ('uploading', 'failed')
              AND offset = expected_size
            "#,
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("upload_session.mark_finalizing", error))?;
        Ok(result.rows_affected() == 1)
    }

    async fn save_actual_checksum(
        &self,
        id: &UploadId,
        checksum: &Checksum,
    ) -> Result<(), CoreError> {
        let result = sqlx::query(
            r#"
            UPDATE upload_sessions
            SET actual_checksum_value = ?, updated_at = ?
            WHERE id = ? AND status = 'finalizing'
            "#,
        )
        .bind(checksum.value())
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("upload_session.save_actual_checksum", error))?;
        if result.rows_affected() != 1 {
            return Err(CoreError::conflict(format!(
                "upload session `{id}` is no longer finalizing"
            )));
        }
        Ok(())
    }

    async fn mark_completed(&self, id: &UploadId) -> Result<(), CoreError> {
        let result = sqlx::query(
            r#"
            UPDATE upload_sessions
            SET status = 'completed', failure = NULL, updated_at = ?
            WHERE id = ?
              AND status = 'finalizing'
              AND offset = expected_size
              AND actual_checksum_value IS NOT NULL
              AND actual_checksum_value = expected_checksum_value
            "#,
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("upload_session.mark_completed", error))?;
        if result.rows_affected() != 1 {
            return Err(CoreError::conflict(format!(
                "upload session `{id}` is no longer finalizing"
            )));
        }
        Ok(())
    }

    async fn mark_failed(&self, id: &UploadId, failure: &str) -> Result<(), CoreError> {
        let result = sqlx::query(
            r#"
            UPDATE upload_sessions
            SET status = 'failed', failure = ?, updated_at = ?
            WHERE id = ? AND status = 'finalizing'
            "#,
        )
        .bind(failure)
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("upload_session.mark_failed", error))?;
        if result.rows_affected() != 1 {
            return Err(CoreError::conflict(format!(
                "upload session `{id}` is no longer finalizing"
            )));
        }
        Ok(())
    }

    async fn list_finalizing(&self) -> Result<Vec<UploadId>, CoreError> {
        let rows = sqlx::query(
            "SELECT id FROM upload_sessions WHERE status = 'finalizing' ORDER BY updated_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("upload_session.list_finalizing", error))?;
        rows.into_iter()
            .map(|row| {
                let value: String = row.get("id");
                UploadId::from_str(&value)
                    .map_err(|error| CoreError::repository("upload_session.id", error))
            })
            .collect()
    }
}

fn decode_session(row: sqlx::sqlite::SqliteRow) -> Result<UploadSession, CoreError> {
    let parse_id = |field: &'static str, value: String| {
        uuid::Uuid::parse_str(&value).map_err(|error| CoreError::repository(field, error))
    };
    let expected_size = decode_u64("upload_session.expected_size", row.get("expected_size"))?;
    let offset = decode_u64("upload_session.offset", row.get("offset"))?;
    let timestamp = |field: &'static str, value: String| {
        DateTime::parse_from_rfc3339(&value)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|error| CoreError::repository(field, error))
    };
    UploadSession::rehydrate(UploadSessionSnapshot {
        id: UploadId::from_uuid(parse_id("upload_session.id", row.get("id"))?),
        resource_id: ResourceId::from_uuid(parse_id(
            "upload_session.resource_id",
            row.get("resource_id"),
        )?),
        owner_id: UserId::from_uuid(parse_id("upload_session.owner_id", row.get("owner_id"))?),
        name: row.get("name"),
        directory: DirectoryPath::from_str(row.get::<String, _>("directory").as_str())
            .map_err(|error| CoreError::repository("upload_session.directory", error))?,
        kind: ResourceKind::from_str(row.get::<String, _>("kind").as_str())
            .map_err(|error| CoreError::repository("upload_session.kind", error))?,
        mime_type: row.get("mime_type"),
        expected_size,
        offset,
        status: decode_status(row.get("status"))?,
        expected_checksum: Checksum::sha256(row.get::<String, _>("expected_checksum_value"))
            .map_err(|error| CoreError::repository("upload_session.expected_checksum", error))?,
        actual_checksum: row
            .get::<Option<String>, _>("actual_checksum_value")
            .map(Checksum::sha256)
            .transpose()
            .map_err(|error| CoreError::repository("upload_session.actual_checksum", error))?,
        failure: row.get("failure"),
        created_at: timestamp("upload_session.created_at", row.get("created_at"))?,
        updated_at: timestamp("upload_session.updated_at", row.get("updated_at"))?,
    })
    .map_err(|error| CoreError::repository("upload_session.rehydrate", error))
}

fn decode_status(value: String) -> Result<UploadStatus, CoreError> {
    match value.as_str() {
        "uploading" => Ok(UploadStatus::Uploading),
        "finalizing" => Ok(UploadStatus::Finalizing),
        "completed" => Ok(UploadStatus::Completed),
        "failed" => Ok(UploadStatus::Failed),
        _ => Err(CoreError::repository(
            "upload_session.status",
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown upload status `{value}`"),
            ),
        )),
    }
}

fn encode_u64(value: u64) -> Result<i64, CoreError> {
    i64::try_from(value)
        .map_err(|_| CoreError::configuration("upload size exceeds SQLite INTEGER range"))
}

fn decode_u64(field: &'static str, value: i64) -> Result<u64, CoreError> {
    u64::try_from(value).map_err(|error| CoreError::repository(field, error))
}
