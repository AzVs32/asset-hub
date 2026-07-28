//! Manifest 中的插件能力声明文档。
//!
//! 这些类型直接映射 manifest JSON。到归一化 Action 领域定义的转换由同级
//! `normalization` 模块实现，避免把文档形状与运行时模型视为同一层对象。

use crate::ResourceContentMatcher;
use serde::{Deserialize, Serialize};

/// Capabilities contributed by a plugin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginCapabilities {
    pub kinds: Vec<ResourceKindCapability>,
    pub directory_kinds: Vec<DirectoryKindCapability>,
    pub actions: Vec<ResourceActionCapability>,
    pub directory_actions: Vec<DirectoryActionCapability>,
}

/// Directory kind contributed by a plugin manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectoryKindCapability {
    pub kind: String,
    pub parent: Option<String>,
    pub label: Option<String>,
}

/// Resource kind contributed by a plugin manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceKindCapability {
    pub kind: String,
    pub parent: Option<String>,
    pub label: Option<String>,
    pub supports_content: bool,
    pub detect: ResourceContentMatcher,
}

impl Default for ResourceKindCapability {
    fn default() -> Self {
        Self {
            kind: String::new(),
            parent: None,
            label: None,
            supports_content: true,
            detect: ResourceContentMatcher::default(),
        }
    }
}

/// Resource action contributed by a plugin manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceActionCapability {
    pub id: String,
    pub label: String,
    pub handler: String,
    #[serde(default)]
    pub applies_to: ActionAppliesTo,
    #[serde(default)]
    pub access: ManifestActionAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<ActionRequirements>,
    pub views: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ActionUi>,
}

/// Directory action contributed by a plugin manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryActionCapability {
    pub id: String,
    pub label: String,
    pub handler: String,
    #[serde(default)]
    pub applies_to: DirectoryActionAppliesToCapability,
    #[serde(default)]
    pub access: ManifestActionAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<DirectoryActionRequirementsCapability>,
    pub views: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ActionUi>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectoryActionAppliesToCapability {
    pub kinds: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectoryActionRequirementsCapability {
    pub children: bool,
    pub resources: bool,
}

/// Manifest-level action access declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ManifestActionAccess {
    #[default]
    Read,
    Write,
}

/// Resource/action matching declaration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionAppliesTo {
    pub kinds: Vec<String>,
    pub media_types: Vec<String>,
    pub extensions: Vec<String>,
}

/// Optional object content a handler needs in addition to the resource snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRequirements {
    #[serde(default)]
    pub content: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_delivery: Option<ContentDelivery>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentDelivery {
    Inline,
    Reference,
}

#[cfg(test)]
mod tests;

/// Optional UI placement hints for host applications.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionUi {
    pub group: Option<String>,
    pub order: Option<i32>,
    pub locations: Vec<String>,
}
