use asset_core::CoreError;
use asset_core::domain::{IdempotencyKey, IdempotencyRecord, IdempotencyStatus};
use asset_core::port::IdempotencyRepository;
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
    async fn find(&self, key: &IdempotencyKey) -> Result<Option<IdempotencyRecord>, CoreError> {
        let row = sqlx::query(
            r#"
            SELECT key, request_hash, status, result_json, created_at, completed_at
            FROM idempotency_records
            WHERE key = ?
            "#,
        )
        .bind(key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoreError::repository("idempotency.find", error))?;
        row.map(decode_record).transpose()
    }

    async fn insert(&self, record: &IdempotencyRecord) -> Result<(), CoreError> {
        let result = sqlx::query(
            r#"
            INSERT INTO idempotency_records (key, request_hash, status, result_json, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(record.key().as_str())
        .bind(record.request_hash())
        .bind(encode_status(record.status()))
        .bind(record.result().map(|value| value.to_string()))
        .bind(record.created_at().to_rfc3339())
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                Err(CoreError::conflict(format!(
                    "idempotency key `{}` is already in use",
                    record.key()
                )))
            }
            Err(error) => Err(CoreError::repository("idempotency.insert", error)),
        }
    }

    async fn complete(
        &self,
        key: &IdempotencyKey,
        result: serde_json::Value,
    ) -> Result<(), CoreError> {
        sqlx::query(
            r#"
            UPDATE idempotency_records
            SET status = ?, result_json = ?, completed_at = ?
            WHERE key = ?
            "#,
        )
        .bind(encode_status(IdempotencyStatus::Completed))
        .bind(result.to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(key.as_str())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|error| CoreError::repository("idempotency.complete", error))
    }

    async fn remove(&self, key: &IdempotencyKey) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM idempotency_records WHERE key = ?")
            .bind(key.as_str())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::repository("idempotency.remove", error))
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
    let result = row
        .get::<Option<String>, _>("result_json")
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|error| CoreError::repository("idempotency.result", error))?;
    let created_at = timestamp("idempotency.created_at", row.get("created_at"))?;
    let completed_at = row
        .get::<Option<String>, _>("completed_at")
        .map(|value| parse_timestamp("idempotency.completed_at", &value))
        .transpose()?;
    Ok(IdempotencyRecord::rehydrate(
        key,
        request_hash,
        status,
        result,
        created_at,
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
