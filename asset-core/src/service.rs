//! 应用服务与用例入口。
//!
//! service 层负责协调领域模型和端口完成完整业务动作。非可信应用入口应通过
//! [`SecuredResourceService`] 只暴露资源元数据/生命周期；内容、上传与动作分别通过各自
//! 的授权门面进入。该层不依赖 OpenDAL、sqlx 等具体基础设施实现。

mod asset;
mod authorization;
mod directory;
mod idempotency;
mod resource;
mod user;

pub use asset::{
    AssetWorkflowService, DirectoryArchiveManifest, DirectoryArchiveResource,
    SecuredAssetWorkflowService,
};
pub use authorization::{AuthorizationService, WorkspaceScope};
pub use directory::{
    DirectoryIndexService, DirectoryProvisioningService, DirectoryService, DirectoryServices,
    SecuredDirectoryService, UpdateDirectory,
};
pub use idempotency::{IdempotencyOutcome, IdempotencyService, request_hash};
pub use user::UserService;

pub use resource::{
    ContentService, CreateUpload, ReplaceResourceContent, ResourceContentStream,
    ResourceScanProgress, ResourceService, ResourceServices, SecuredContentService,
    SecuredResourceService, SecuredUploadService, StorageMaintenanceService,
    StorageReconciliationReport, UpdateResource, UploadService,
};
