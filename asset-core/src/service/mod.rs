//! 应用服务与用例入口。
//!
//! service 层负责协调领域模型和端口完成完整业务动作。非可信应用入口应通过
//! [`SecuredResourceService`] 调用资源用例；未绑定授权上下文的子服务仅在 Core 内部可见。
//!
//! 该层不依赖 OpenDAL、sqlx 等具体基础设施实现；调用方需要在应用启动时注入
//! `ResourceRepository` 和 `BlobStorage` 的具体适配器。

mod authorization_service;
mod resource_service;
mod user_service;

pub use authorization_service::AuthorizationService;
pub use user_service::UserService;

pub use resource_service::{
    CreateResource, ExecuteResourceAction, ReadableResource, ResourceActions,
    ResourceContentCommand, ResourceContentStream, ResourcePreviewStream, ResourceService,
    ResourceServicePorts, ResourceThumbnail, SecuredResourceService, UpdateResource,
    UploadResourceContentStream,
};
