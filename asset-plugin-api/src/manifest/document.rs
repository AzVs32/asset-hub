//! 完整插件 Manifest 的 JSON 根文档。
//!
//! `PluginManifest` 聚合作者声明的身份、运行时、能力、权限和 Web 资产配置；文档允许
//! 先反序列化再统一校验，跨字段不变量由同级 `validation` 模块实施。

use super::{PluginCapabilities, PluginDescriptor, PluginPermissions, PluginRuntime, PluginWeb};
use serde::{Deserialize, Serialize};

/// Current and only supported manifest schema version.
pub const MANIFEST_VERSION: u32 = 3;
pub const PLUGIN_API_VERSION: &str = "asset-hub.plugin-api@0.4";

/// Complete plugin manifest document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "$schema")]
    pub schema: Option<String>,
    pub manifest_version: u32,
    pub plugin: PluginDescriptor,
    pub runtime: PluginRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<PluginWeb>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    pub permissions: PluginPermissions,
}

impl PluginManifest {
    pub fn plugin_id(&self) -> &str {
        &self.plugin.id
    }
}
