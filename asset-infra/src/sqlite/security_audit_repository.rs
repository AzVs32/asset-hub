use asset_core::{
    CoreError,
    domain::{
        NewSecurityAuditEvent, SecurityAuditActor, SecurityAuditEvent, SecurityAuditEventType,
        SecurityAuditOutcome, SecurityAuditSource, UserId,
    },
    port::SecurityAuditRepository,
};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use std::{io, str::FromStr};

#[derive(Clone)]
pub struct SqliteSecurityAuditRepository {
    pool: SqlitePool,
}

impl SqliteSecurityAuditRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl SecurityAuditRepository for SqliteSecurityAuditRepository {
    async fn record(&self, event: &NewSecurityAuditEvent) -> Result<(), CoreError> {
        let actor_user_id = event.actor.user_id().map(|id| id.to_string());

        sqlx::query(
            r#"
            INSERT INTO security_audit_events (
                occurred_at, actor_user_id, source, event_type, outcome, target
            ) VALUES (
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?, ?, ?, ?, ?
            )
            "#,
        )
        .bind(actor_user_id)
        .bind(event.source.as_str())
        .bind(event.event_type.as_str())
        .bind(event.outcome.as_str())
        .bind(&event.target)
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("security_audit.record", error))?;

        Ok(())
    }

    async fn list(&self, limit: u32, offset: u64) -> Result<Vec<SecurityAuditEvent>, CoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, occurred_at, actor_user_id, source, event_type, outcome, target
            FROM security_audit_events
            ORDER BY occurred_at DESC, id DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(i64::from(limit))
        .bind(i64::try_from(offset).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("security_audit.list", error))?;

        rows.into_iter().map(decode_event).collect()
    }
}

fn decode_event(row: SqliteRow) -> Result<SecurityAuditEvent, CoreError> {
    let actor_user_id = row
        .try_get::<Option<String>, _>("actor_user_id")
        .map_err(decode_error)?;
    let actor = match actor_user_id {
        Some(user_id) => {
            SecurityAuditActor::authenticated(UserId::from_str(&user_id).map_err(decode_error)?)
        }
        None => SecurityAuditActor::unauthenticated(),
    };

    let source_name = row.try_get::<String, _>("source").map_err(decode_error)?;
    let source = SecurityAuditSource::from_stable_str(&source_name)
        .ok_or_else(|| invalid_data(format!("unknown security audit source `{source_name}`")))?;
    let outcome_name = row.try_get::<String, _>("outcome").map_err(decode_error)?;
    let outcome = SecurityAuditOutcome::from_stable_str(&outcome_name)
        .ok_or_else(|| invalid_data(format!("unknown security audit outcome `{outcome_name}`")))?;

    let event_type_name = row
        .try_get::<String, _>("event_type")
        .map_err(decode_error)?;
    let event_type =
        SecurityAuditEventType::from_stable_str(&event_type_name).ok_or_else(|| {
            invalid_data(format!(
                "unknown security audit event type `{event_type_name}`"
            ))
        })?;
    let occurred_at = DateTime::parse_from_rfc3339(
        &row.try_get::<String, _>("occurred_at")
            .map_err(decode_error)?,
    )
    .map(|value| value.with_timezone(&Utc))
    .map_err(decode_error)?;

    Ok(SecurityAuditEvent {
        id: row.try_get("id").map_err(decode_error)?,
        occurred_at,
        actor,
        source,
        event_type,
        outcome,
        target: row.try_get("target").map_err(decode_error)?,
    })
}

fn decode_error(error: impl std::error::Error + Send + Sync + 'static) -> CoreError {
    CoreError::repository("security_audit.decode", error)
}

fn invalid_data(message: impl Into<String>) -> CoreError {
    decode_error(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

#[cfg(test)]
mod tests;
