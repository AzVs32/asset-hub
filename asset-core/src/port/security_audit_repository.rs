use crate::{
    CoreError,
    domain::{NewSecurityAuditEvent, SecurityAuditEvent},
};

#[async_trait::async_trait]
pub trait SecurityAuditRepository: Send + Sync {
    async fn record(&self, event: &NewSecurityAuditEvent) -> Result<(), CoreError>;
    async fn list(&self, limit: u32, offset: u64) -> Result<Vec<SecurityAuditEvent>, CoreError>;
}
