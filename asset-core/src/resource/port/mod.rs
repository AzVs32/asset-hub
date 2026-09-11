//! 资源聚合持久化、读取模型与生命周期状态端口。

mod deletion;
mod persistence;
mod replacement;
mod upload;

pub use deletion::ResourceDeletionStore;
pub use persistence::{
    ResourceMaintenanceReadModel, ResourceReadModel, ResourceRelocation, ResourceRelocationStore,
    ResourceStore,
};
pub use replacement::ResourceContentReplacementStore;
pub use upload::UploadSessionStore;
