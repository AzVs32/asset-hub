//! 完整插件 Manifest 的 JSON 根文档。
//!
//! `PluginManifestDocument` 聚合作者声明的身份、运行时、能力和权限；文档允许
//! 先反序列化再统一校验，跨字段不变量由同级 `validation` 模块实施。

use super::{
    ManifestValidationError, PluginCapabilities, PluginDescriptor, PluginPermissions, PluginRuntime,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Deref;

/// Current and only supported manifest schema version.
pub const MANIFEST_VERSION: u32 = 5;
pub const PLUGIN_MANIFEST_FILE_NAME: &str = "manifest.json";
pub const PLUGIN_LOCK_FILE_NAME: &str = "manifest.lock.json";
pub const PLUGIN_WASM_FILE_NAME: &str = "plugin.wasm";
pub const PLUGIN_WEB_ENTRY_FILE_NAME: &str = "index.html";

/// Complete plugin manifest document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginManifestDocument {
    pub manifest_version: u32,
    pub plugin: PluginDescriptor,
    pub runtime: PluginRuntime,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    pub permissions: PluginPermissions,
}

impl PluginManifestDocument {
    pub fn plugin_id(&self) -> &str {
        &self.plugin.id
    }

    pub fn validate(self) -> Result<ValidatedPluginManifest, ManifestValidationError> {
        super::validation::validate_manifest(&self)?;
        Ok(ValidatedPluginManifest(self))
    }
}

/// Manifest that has passed all schema-level and cross-field invariants.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ValidatedPluginManifest(PluginManifestDocument);

impl ValidatedPluginManifest {
    pub fn as_document(&self) -> &PluginManifestDocument {
        &self.0
    }

    pub fn into_document(self) -> PluginManifestDocument {
        self.0
    }

    pub fn plugin_id(&self) -> &str {
        self.0.plugin_id()
    }
}

impl Deref for ValidatedPluginManifest {
    type Target = PluginManifestDocument;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
