//! 资源聚合、查询、类型与动作运行时端口。

mod action;
mod kind;
mod persistence;

pub use action::{
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRegistry, ResourceActionRequest,
};
pub use kind::{ResourceKindDefinition, ResourceKindRegistry};
pub use persistence::{ListResources, ResourcePage, ResourceQuery, ResourceRepository};

#[cfg(test)]
mod tests;
