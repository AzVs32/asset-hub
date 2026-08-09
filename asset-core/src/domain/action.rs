//! Host 内部的 Action 注册、匹配、授权与执行模型。
//!
//! 外部 Manifest 由基础设施适配器显式转换为这些定义；本模块不属于插件 SDK，
//! 也不描述 Host 与插件之间的 JSON 线协议。

mod common;
mod directory;
mod matcher;
mod resource;

pub use common::{
    ActionAccess, ActionCapabilityId, ActionDefinition, ActionId, ActionIdError,
    ActionOutputContract, ActionUi,
};
pub use directory::{
    DirectoryActionAppliesTo, DirectoryActionDefinition, DirectoryActionId,
    DirectoryActionRequirements,
};
pub use matcher::ResourceContentMatcher;
pub use resource::{
    ResourceActionAppliesTo, ResourceActionContentDelivery, ResourceActionDefinition,
    ResourceActionId, ResourceActionRequirements,
};

#[cfg(test)]
mod tests;
