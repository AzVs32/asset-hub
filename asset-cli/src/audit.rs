use asset_core::CoreError;
use asset_core::domain::{
    NewSecurityAuditEvent, SecurityAuditActor, SecurityAuditEventType, SecurityAuditOutcome,
    SecurityAuditSource,
};
use asset_core::port::SecurityAuditRepository;
use std::future::Future;

/// 执行 CLI 敏感操作，并以 fail-open 方式记录其成功或失败结果。
///
/// CLI 当前没有登录会话，因此操作者保持为未认证；`target` 只能包含操作目标的非敏感标识，
/// 不得传入密码、令牌或请求内容。审计写入失败会输出诊断，但不会覆盖业务操作结果。
pub(crate) async fn audited<T>(
    repository: &dyn SecurityAuditRepository,
    event_type: SecurityAuditEventType,
    target: Option<&str>,
    operation: impl Future<Output = Result<T, CoreError>>,
) -> Result<T, CoreError> {
    let result = operation.await;
    let event = NewSecurityAuditEvent {
        actor: SecurityAuditActor::unauthenticated(),
        source: SecurityAuditSource::Cli,
        event_type,
        outcome: if result.is_ok() {
            SecurityAuditOutcome::Success
        } else {
            SecurityAuditOutcome::Failure
        },
        target: target.map(str::trim).map(ToOwned::to_owned),
    };
    if let Err(error) = repository.record(&event).await {
        eprintln!("asset: failed to record security audit event: {error}");
    }
    result
}

#[cfg(test)]
mod tests;
