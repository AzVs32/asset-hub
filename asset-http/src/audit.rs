use asset_core::domain::SecurityAuditEvent;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SecurityAuditEventResponse {
    pub(crate) id: i64,
    pub(crate) occurred_at: String,
    pub(crate) actor_user_id: Option<String>,
    pub(crate) source: String,
    pub(crate) event_type: String,
    pub(crate) outcome: String,
    pub(crate) target: Option<String>,
}

impl From<SecurityAuditEvent> for SecurityAuditEventResponse {
    fn from(event: SecurityAuditEvent) -> Self {
        Self {
            id: event.id,
            occurred_at: event.occurred_at.to_rfc3339(),
            actor_user_id: event.actor.user_id().map(|id| id.to_string()),
            source: event.source.as_str().to_string(),
            event_type: event.event_type.as_str().to_string(),
            outcome: event.outcome.as_str().to_string(),
            target: event.target,
        }
    }
}
