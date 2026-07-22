//! 安全审计事件持久化端口。

use crate::{
    CoreError,
    domain::{NewSecurityAuditEvent, SecurityAuditEvent},
};

/// 保存和分页读取不可变安全审计事件。
#[async_trait::async_trait]
pub trait SecurityAuditRepository: Send + Sync {
    async fn record(&self, event: &NewSecurityAuditEvent) -> Result<(), CoreError>;
    async fn list(&self, limit: u32, offset: u64) -> Result<Vec<SecurityAuditEvent>, CoreError>;
}
