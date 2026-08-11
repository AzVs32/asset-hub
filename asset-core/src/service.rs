//! 应用服务与用例入口。
//!
//! service 层负责协调领域模型和端口完成完整业务动作。非可信应用入口应通过
//! [`SecuredResourceService`] 调用资源用例；未绑定授权上下文的子服务仅在 Core 内部可见。
//!
//! 该层不依赖 OpenDAL、sqlx 等具体基础设施实现；调用方需要在应用启动时注入
//! [`ResourceServicePorts`] 所声明的写仓储、查询、Blob、扫描与运行时适配器，并注入共享
//! 的 [`DirectoryService`]。

mod authorization;
mod directory;
mod resource;
mod user;

pub use authorization::{AuthorizationService, WorkspaceScope};
pub use directory::{
    DirectoryActions, DirectoryService, ExecuteDirectoryAction, SecuredDirectoryService,
    UpdateDirectory,
};
pub use user::UserService;

pub use resource::{
    CreateUpload, ExecuteResourceAction, ReplaceResourceContent, ResourceActions,
    ResourceContentStream, ResourceScanProgress, ResourceService, ResourceServicePorts,
    SecuredResourceService, StorageReconciliationReport, UpdateResource,
};

use crate::{CoreError, domain::ActionAccess};

/// Enforce optimistic concurrency only when the action contract needs it.
///
/// Write actions always require a caller revision. Read actions may omit it to operate on the
/// latest authorized snapshot; when supplied, it remains an explicit consistency precondition.
fn validate_action_revision(
    access: ActionAccess,
    expected_revision: Option<u64>,
    actual_revision: u64,
    aggregate: &'static str,
    id: impl Into<String>,
) -> Result<(), CoreError> {
    if access == ActionAccess::Write && expected_revision.is_none() {
        return Err(CoreError::invalid_operation(
            "expected_revision is required for write actions",
        ));
    }
    if expected_revision.is_some_and(|expected| expected != actual_revision) {
        return Err(CoreError::revision_conflict(aggregate, id));
    }
    Ok(())
}
