//! 应用服务与用例入口。
//!
//! service 层负责协调领域模型和端口完成完整业务动作。`ResourceService`、
//! `DirectoryService`、`ContentService` 和 `AssetWorkflowService` 提供无用户上下文的资源、
//! 全局目录、内容和跨聚合投影用例；`UploadService` 也提供无用户上下文的上传用例。该层不
//! 依赖 OpenDAL、sqlx 等具体基础设施实现。

mod asset;
mod directory;
mod idempotency;
mod resource;

pub use asset::{AssetWorkflowService, DirectoryArchiveManifest, DirectoryArchiveResource};
pub use directory::{
    DirectoryImportService, DirectoryIndexService, DirectoryService, DirectoryServices,
    UpdateDirectory,
};
pub use idempotency::{IdempotencyOutcome, IdempotencyService, request_hash};
pub use resource::{
    ContentService, CreateUpload, ReplaceResourceContent, ResourceContentStream,
    ResourceScanProgress, ResourceService, ResourceServices, StorageMaintenanceService,
    StorageReconciliationReport, UpdateResource, UploadService,
};
