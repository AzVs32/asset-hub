use asset_core::CoreError;
use asset_core::domain::{
    IdempotencyExecutionId, IdempotencyKey, IdempotencyRecord, IdempotencyStatus,
};
use asset_core::port::{IdempotencyAcquire, IdempotencyRepository};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

#[derive(Clone)]
pub struct SqliteIdempotencyRepository {
    pool: SqlitePool,
}

impl SqliteIdempotencyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl IdempotencyRepository for SqliteIdempotencyRepository {
    async fn acquire(&self, record: &IdempotencyRecord) -> Result<IdempotencyAcquire, CoreError> {
        // One conditional UPSERT is the compare-and-set. SQLite serializes this write statement,
        // so two expired-lease callers cannot both become owners. The `WHERE` is evaluated while
        // applying the conflicting row, rather than against a stale prior SELECT.
        let result = sqlx::query(
            r#"
            INSERT INTO idempotency_records (
                key, request_hash, status, execution_id, lease_expires_at, result_json,
                created_at, updated_at, completed_at
            )
            VALUES (?, ?, ?, ?, ?, NULL, ?, ?, NULL)
            ON CONFLICT(key) DO UPDATE SET
                execution_id = excluded.execution_id,
                lease_expires_at = excluded.lease_expires_at,
                updated_at = excluded.updated_at
            WHERE idempotency_records.request_hash = excluded.request_hash
              AND idempotency_records.status = 'in_progress'
              AND idempotency_records.lease_expires_at <= excluded.updated_at
            "#,
        )
        .bind(record.key().as_str())
        .bind(record.request_hash())
        .bind(encode_status(record.status()))
        .bind(record.execution_id().to_string())
        .bind(record.lease_expires_at().to_rfc3339())
        .bind(record.created_at().to_rfc3339())
        .bind(record.updated_at().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("idempotency.acquire", error))?;

        if result.rows_affected() == 1 {
            return Ok(IdempotencyAcquire::Acquired);
        }

        self.find_existing(record.key()).await
    }

    async fn complete(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
        result: serde_json::Value,
    ) -> Result<bool, CoreError> {
        let updated = sqlx::query(
            r#"
            UPDATE idempotency_records
            SET status = ?, result_json = ?, updated_at = ?, completed_at = ?
            WHERE key = ? AND execution_id = ? AND status = 'in_progress'
            "#,
        )
        .bind(encode_status(IdempotencyStatus::Completed))
        .bind(result.to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(key.as_str())
        .bind(execution_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("idempotency.complete", error))?;
        Ok(updated.rows_affected() == 1)
    }

    async fn remove(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
    ) -> Result<bool, CoreError> {
        let deleted = sqlx::query(
            "DELETE FROM idempotency_records WHERE key = ? AND execution_id = ? AND status = 'in_progress'",
        )
        .bind(key.as_str())
        .bind(execution_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("idempotency.remove", error))?;
        Ok(deleted.rows_affected() == 1)
    }

    async fn renew(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<bool, CoreError> {
        let updated = sqlx::query(
            r#"
            UPDATE idempotency_records
            SET lease_expires_at = ?, updated_at = ?
            WHERE key = ? AND execution_id = ? AND status = 'in_progress'
            "#,
        )
        .bind(lease_expires_at.to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(key.as_str())
        .bind(execution_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("idempotency.renew", error))?;
        Ok(updated.rows_affected() == 1)
    }
}

impl SqliteIdempotencyRepository {
    async fn find_existing(&self, key: &IdempotencyKey) -> Result<IdempotencyAcquire, CoreError> {
        let row = sqlx::query(
            r#"
            SELECT key, request_hash, status, execution_id, lease_expires_at, result_json,
                   created_at, updated_at, completed_at
            FROM idempotency_records
            WHERE key = ?
            "#,
        )
        .bind(key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoreError::repository("idempotency.acquire.read", error))?;
        Ok(match row {
            Some(row) => IdempotencyAcquire::Existing(decode_record(row)?),
            None => IdempotencyAcquire::Vacant,
        })
    }
}

fn encode_status(status: IdempotencyStatus) -> &'static str {
    match status {
        IdempotencyStatus::InProgress => "in_progress",
        IdempotencyStatus::Completed => "completed",
    }
}

fn decode_record(row: sqlx::sqlite::SqliteRow) -> Result<IdempotencyRecord, CoreError> {
    let key = IdempotencyKey::from_str(row.get::<String, _>("key").as_str())
        .map_err(|error| CoreError::repository("idempotency.key", error))?;
    let request_hash = row.get::<String, _>("request_hash");
    let status = decode_status(row.get::<String, _>("status"))?;
    let execution_id = IdempotencyExecutionId::parse(&row.get::<String, _>("execution_id"))
        .map_err(|error| CoreError::repository("idempotency.execution_id", error))?;
    let lease_expires_at = timestamp("idempotency.lease_expires_at", row.get("lease_expires_at"))?;
    let result = row
        .get::<Option<String>, _>("result_json")
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|error| CoreError::repository("idempotency.result", error))?;
    let created_at = timestamp("idempotency.created_at", row.get("created_at"))?;
    let updated_at = timestamp("idempotency.updated_at", row.get("updated_at"))?;
    let completed_at = row
        .get::<Option<String>, _>("completed_at")
        .map(|value| parse_timestamp("idempotency.completed_at", &value))
        .transpose()?;
    Ok(IdempotencyRecord::rehydrate(
        key,
        request_hash,
        status,
        execution_id,
        lease_expires_at,
        result,
        created_at,
        updated_at,
        completed_at,
    ))
}

fn decode_status(value: String) -> Result<IdempotencyStatus, CoreError> {
    match value.as_str() {
        "in_progress" => Ok(IdempotencyStatus::InProgress),
        "completed" => Ok(IdempotencyStatus::Completed),
        _ => Err(CoreError::invariant(format!(
            "unknown idempotency status `{value}`"
        ))),
    }
}

fn timestamp(field: &'static str, value: String) -> Result<DateTime<Utc>, CoreError> {
    parse_timestamp(field, &value)
}

fn parse_timestamp(field: &'static str, value: &str) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| CoreError::repository(field, error))
}

#[cfg(test)]
mod tests;
