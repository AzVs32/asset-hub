//! 应用服务与用例入口。
//!
//! service 层负责协调领域模型和端口完成一个完整业务动作。这里的公开方法就是当前
//! 核心层对外提供的 usecase，例如创建资源、上传内容、读取内容、软删除和物理移除。
//!
//! 该层不依赖 OpenDAL、sqlx 等具体基础设施实现；调用方需要在应用启动时注入
//! `ResourceRepository` 和 `BlobStorage` 的具体适配器。

mod authorization_service;
mod resource_service;
mod user_service;

pub use authorization_service::AuthorizationService;
pub use user_service::UserService;

pub use resource_service::{
    CreateResource, ExecuteResourceAction, ImportResourceContent, ReadableResource,
    ResourceActionService, ResourceActions, ResourceCommandService, ResourceContentService,
    ResourcePreview, ResourcePreviewService, ResourceService, ResourceThumbnail,
    SecuredResourceService, UpdateResource, UploadResourceContent, UploadResourceContentStream,
};
