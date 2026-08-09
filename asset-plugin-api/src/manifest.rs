//! 外部插件 Manifest 文档契约及其校验入口。
//!
//! Manifest 是插件包的外部声明文档：`document` 定义根文档，`capability`、
//! `permission`、`runtime` 等模块定义各字段形状，`validation` 负责跨字段约束。
//! Manifest 到 Host 内部模型的转换由 Host 适配器负责，不属于 SDK。

mod capability;
mod descriptor;
mod document;
mod lock;
mod permission;
mod runtime;
mod validation;

pub use capability::{
    ActionOutputCapability, ActionRequirements, ActionUi, ContentDelivery,
    DirectoryActionAppliesToCapability, DirectoryActionCapability,
    DirectoryActionRequirementsCapability, DirectoryKindCapability, ManifestActionAccess,
    PluginCapabilities, ResourceActionAppliesToCapability, ResourceActionCapability,
    ResourceContentMatcher, ResourceKindCapability,
};
pub use descriptor::PluginDescriptor;
pub use document::{
    MANIFEST_VERSION, PLUGIN_LOCK_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME, PLUGIN_WASM_FILE_NAME,
    PLUGIN_WEB_ENTRY_FILE_NAME, PluginManifest,
};
pub use lock::PluginManifestLock;
pub use permission::{
    FilesystemPermission, NetworkPermission, PluginPermission, PluginPermissions,
};
pub use runtime::PluginRuntime;

#[cfg(test)]
mod tests;
