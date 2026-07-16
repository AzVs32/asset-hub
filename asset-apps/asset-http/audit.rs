use serde::Serialize;
use tower_sessions_sqlx_store::sqlx;
use tower_sessions_sqlx_store::sqlx::{Row, SqlitePool};
use utoipa::ToSchema;

#[derive(Clone)]
pub(crate) struct SecurityAuditLog {
    pool: SqlitePool,
}

pub(crate) struct NewSecurityAuditEvent<'a> {
    pub(crate) actor_user_id: Option<String>,
    pub(crate) actor_username: Option<String>,
    pub(crate) event_type: &'a str,
    pub(crate) method: &'a str,
    pub(crate) path: &'a str,
    pub(crate) status_code: u16,
    pub(crate) target: Option<&'a str>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SecurityAuditEventResponse {
    pub(crate) id: i64,
    pub(crate) occurred_at: String,
    pub(crate) actor_user_id: Option<String>,
    pub(crate) actor_username: Option<String>,
    pub(crate) event_type: String,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) status_code: u16,
    pub(crate) outcome: String,
    pub(crate) target: Option<String>,
}

impl SecurityAuditLog {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub(crate) async fn record(
        &self,
        event: NewSecurityAuditEvent<'_>,
    ) -> Result<(), tower_sessions_sqlx_store::sqlx::Error> {
        let outcome = if event.status_code < 400 {
            "success"
        } else {
            "failure"
        };
        sqlx::query(
            r#"
            INSERT INTO security_audit_events (
                occurred_at, actor_user_id, actor_username, event_type,
                method, path, status_code, outcome, target
            ) VALUES (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(event.actor_user_id)
        .bind(event.actor_username)
        .bind(event.event_type)
        .bind(event.method)
        .bind(event.path)
        .bind(i64::from(event.status_code))
        .bind(outcome)
        .bind(event.target)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn list(
        &self,
        limit: u32,
        offset: u64,
    ) -> Result<Vec<SecurityAuditEventResponse>, tower_sessions_sqlx_store::sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, occurred_at, actor_user_id, actor_username, event_type,
                   method, path, status_code, outcome, target
            FROM security_audit_events
            ORDER BY occurred_at DESC, id DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(i64::from(limit))
        .bind(i64::try_from(offset).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let status_code = row.try_get::<i64, _>("status_code")?;
                Ok(SecurityAuditEventResponse {
                    id: row.try_get("id")?,
                    occurred_at: row.try_get("occurred_at")?,
                    actor_user_id: row.try_get("actor_user_id")?,
                    actor_username: row.try_get("actor_username")?,
                    event_type: row.try_get("event_type")?,
                    method: row.try_get("method")?,
                    path: row.try_get("path")?,
                    status_code: u16::try_from(status_code).unwrap_or(u16::MAX),
                    outcome: row.try_get("outcome")?,
                    target: row.try_get("target")?,
                })
            })
            .collect()
    }
}
