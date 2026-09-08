//! 资源聚合、查询与类型运行时端口。

mod deletion;
mod persistence;
mod replacement;
mod upload;

pub use deletion::ResourceDeletionRepository;
pub use persistence::{
    ListResources, LocatedResource, ResourceMaintenanceReadModel, ResourcePage, ResourceReadModel,
    ResourceRelocation, ResourceRelocationStore, ResourceStore,
};
pub use replacement::ResourceContentReplacementRepository;
pub use upload::UploadSessionRepository;
