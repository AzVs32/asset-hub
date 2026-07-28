//! 安全审计事件持久化端口。

use crate::{
    CoreError,
    domain::{NewSecurityAuditEvent, SecurityAuditEvent},
};

/// 保存和分页读取不可变安全审计事件的持久化端口。
///
/// 基础设施适配器必须将事件作为追加记录保存，不得通过该端口修改既有审计事件。
#[async_trait::async_trait]
pub trait SecurityAuditRepository: Send + Sync {
    /// 追加一条审计事件；持久化时间和记录 ID 由适配器生成。
    async fn record(&self, event: &NewSecurityAuditEvent) -> Result<(), CoreError>;

    /// 按最近发生优先的稳定顺序分页读取事件。
    async fn list(&self, limit: u32, offset: u64) -> Result<Vec<SecurityAuditEvent>, CoreError>;
}
