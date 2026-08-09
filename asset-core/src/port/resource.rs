//! 资源聚合、查询、类型与动作运行时端口。

mod action;
mod kind;
mod persistence;
mod replacement;

pub use action::{
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRegistry, ResourceActionRequest,
};
pub use kind::ResourceKindRegistry;
pub use persistence::{
    ListResources, LocatedResource, ResourcePage, ResourceQuery, ResourceRepository,
};
pub use replacement::ResourceContentReplacementRepository;

#[cfg(test)]
mod tests;
