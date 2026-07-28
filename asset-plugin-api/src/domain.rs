//! 插件子域中经过归一化、供 Host 使用的共享领域模型。
//!
//! 当前领域核心是 Action：Manifest capability 会转换为 Action 定义，随后由 Host
//! 注册、匹配和执行。线协议 DTO、Manifest 文档形状及 Wasm ABI 不属于本层。

pub mod action;

pub use action::{
    ActionAccess, ActionDefinition, ActionExecutorKind, ActionId, ActionOutputContract,
    ActionUi as ActionDefinitionUi, DirectoryAction, DirectoryActionAccess,
    DirectoryActionAppliesTo, DirectoryActionDefinition, DirectoryActionExecutorKind,
    DirectoryActionOutputContract, DirectoryActionRequirements, DirectoryActionUi, ResourceAction,
    ResourceActionAccess, ResourceActionAppliesTo, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionExecutorKind, ResourceActionOutputContract,
    ResourceActionRequirements, ResourceActionUi, ResourceContentMatcher,
};
