//! 应用服务与用例入口。
//!
//! service 层负责协调领域模型和端口完成完整业务动作。非可信应用入口应通过
//! [`SecuredResourceService`] 调用资源用例；未绑定授权上下文的子服务仅在 Core 内部可见。
//!
//! 该层不依赖 OpenDAL、sqlx 等具体基础设施实现；调用方需要在应用启动时注入
//! [`ResourceServicePorts`] 所声明的写仓储、查询、Blob、目录、扫描与运行时适配器。

mod authorization;
mod directory;
mod resource;
mod user;

pub use authorization::{AuthorizationService, WorkspaceScope};
pub use directory::{DirectoryActions, DirectoryService, ExecuteDirectoryAction, UpdateDirectory};
pub use user::UserService;

pub use resource::{
    CreateResource, ExecuteResourceAction, ResourceActions, ResourceContentCommand,
    ResourceContentStream, ResourceService, ResourceServicePorts, SecuredResourceService,
    StorageReconciliationReport, UpdateResource, UploadResourceContentStream,
};
