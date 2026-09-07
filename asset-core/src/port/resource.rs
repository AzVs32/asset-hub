//! 资源聚合、查询与类型运行时端口。

mod persistence;
mod replacement;

pub use persistence::{
    ListResources, LocatedResource, ResourceMaintenanceReadModel, ResourcePage, ResourceReadModel,
    ResourceRelocation, ResourceRelocationStore, ResourceStore,
};
pub use replacement::ResourceContentReplacementRepository;
