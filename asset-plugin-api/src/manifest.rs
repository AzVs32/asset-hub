//! 插件 Manifest 文档契约及其校验、归一化入口。
//!
//! Manifest 是插件包的外部声明文档：`document` 定义根文档，`capability`、
//! `permission`、`runtime` 等模块定义各字段形状，`validation` 负责跨字段约束，
//! `normalization` 负责将 capability 转换为 [`crate::domain`] 中的运行时领域模型。

mod capability;
mod descriptor;
mod document;
mod lock;
mod normalization;
mod permission;
mod runtime;
mod validation;

pub use capability::{
    ActionAppliesTo, ActionRequirements, ActionUi, ContentDelivery,
    DirectoryActionAppliesToCapability, DirectoryActionCapability,
    DirectoryActionRequirementsCapability, DirectoryKindCapability, ManifestActionAccess,
    PluginCapabilities, ResourceActionCapability, ResourceKindCapability,
};
pub use descriptor::PluginDescriptor;
pub use document::{
    MANIFEST_VERSION, PLUGIN_API_VERSION, PLUGIN_LOCK_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME,
    PLUGIN_WASM_FILE_NAME, PLUGIN_WEB_ENTRY_FILE_NAME, PluginManifest,
};
pub use lock::PluginManifestLock;
pub use permission::{
    FilesystemPermission, NetworkPermission, PluginPermission, PluginPermissions,
};
pub use runtime::PluginRuntime;
pub use web_assets::PluginWebAssets;

mod web_assets {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Browser assets loaded from verified plugin packages, grouped by plugin id
    /// and package-relative path.
    pub type PluginWebAssets = HashMap<String, HashMap<PathBuf, Arc<[u8]>>>;
}

#[cfg(test)]
mod tests;
