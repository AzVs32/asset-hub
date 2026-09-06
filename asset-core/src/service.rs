//! 应用服务与用例入口。
//!
//! service 层负责协调领域模型和端口完成完整业务动作。`ResourceService`、
//! `DirectoryService`、`ContentService` 和 `AssetWorkflowService` 提供无用户上下文的资源、
//! 全局目录、内容和跨聚合投影用例；非可信应用入口必须分别通过对应的授权包装完成目录授权
//! 后再调用它们。`UploadService` 也提供无用户上下文的上传用例。该层不依赖 OpenDAL、sqlx
//! 等具体基础设施实现。

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
    SecuredResourceService, StorageMaintenanceService, StorageReconciliationReport, UpdateResource,
    UploadService,
};
