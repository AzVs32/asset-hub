//! 应用服务与用例入口。
//!
//! service 层负责协调领域模型和端口完成完整业务动作。非可信应用入口应通过
//! [`SecuredResourceService`] 只暴露资源元数据/生命周期；内容、上传与动作分别通过各自
//! 的授权门面进入。该层不依赖 OpenDAL、sqlx 等具体基础设施实现。

mod asset;
mod authorization;
mod directory;
mod resource;
mod user;

pub use asset::{
    AssetWorkflowService, DirectoryArchiveManifest, DirectoryArchiveResource,
    SecuredAssetWorkflowService,
};
pub use authorization::{AuthorizationService, WorkspaceScope};
pub use directory::{
    DirectoryActions, DirectoryIndexService, DirectoryProvisioningService, DirectoryService,
    DirectoryServices, ExecuteDirectoryAction, SecuredDirectoryService, UpdateDirectory,
};
pub use user::UserService;

pub use resource::{
    ActionOrchestrator, ContentService, CreateUpload, ExecuteResourceAction,
    ReplaceResourceContent, ResourceActions, ResourceContentStream, ResourceScanProgress,
    ResourceService, ResourceServices, SecuredActionOrchestrator, SecuredContentService, SecuredResourceService,
    SecuredUploadService, StorageMaintenanceService, StorageReconciliationReport, UpdateResource,
    UploadService,
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
