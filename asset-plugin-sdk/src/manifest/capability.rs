//! Manifest 中的插件能力声明文档。
//!
//! 这些类型直接映射外部插件的 manifest JSON。Host 如何注册、匹配和执行这些
//! 声明不属于 SDK，由 Host 侧适配器完成转换。

use serde::{Deserialize, Deserializer, Serialize};

/// Resource thumbnail singleton capability declared by a Resource Action provider.
pub const RESOURCE_THUMBNAIL_CAPABILITY: &str = "thumbnail";
/// Read-only frame singleton capability declared by a Resource Action provider.
pub const RESOURCE_VIEW_CAPABILITY: &str = "view";
/// Editor frame singleton capability declared by a Resource Action provider.
pub const RESOURCE_EDIT_CAPABILITY: &str = "edit";
/// Resource Action singleton capability IDs supported by Manifest version 3.
pub const RESOURCE_ACTION_CAPABILITIES: &[&str] = &[
    RESOURCE_THUMBNAIL_CAPABILITY,
    RESOURCE_VIEW_CAPABILITY,
    RESOURCE_EDIT_CAPABILITY,
];

/// Directory thumbnail singleton capability declared by a Directory Action provider.
pub const DIRECTORY_THUMBNAIL_CAPABILITY: &str = "thumbnail";
/// Directory workspace singleton capability declared by a Directory Action provider.
pub const DIRECTORY_WORKSPACE_CAPABILITY: &str = "workspace";
/// Directory Action singleton capability IDs supported by Manifest version 3.
pub const DIRECTORY_ACTION_CAPABILITIES: &[&str] = &[
    DIRECTORY_THUMBNAIL_CAPABILITY,
    DIRECTORY_WORKSPACE_CAPABILITY,
];

/// Capabilities contributed by a plugin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginCapabilities {
    pub resource_kinds: Vec<ResourceKindCapability>,
    pub directory_kinds: Vec<DirectoryKindCapability>,
    pub resource_actions: Vec<ResourceActionCapability>,
    pub directory_actions: Vec<DirectoryActionCapability>,
}

/// Directory kind contributed by a plugin manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectoryKindCapability {
    pub kind: String,
    pub parent: Option<String>,
    pub label: Option<String>,
    /// Kind automatically assigned to otherwise-generic direct child Directories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_child_kind: Option<String>,
    /// Direct parent Directory kinds accepted by this kind.
    ///
    /// An empty list declares no constraint at this node; the nearest ancestor declaration may
    /// still supply the effective constraint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_parent_kinds: Vec<String>,
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
    /// Optional singleton Host capability implemented by this action provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provides: Option<String>,
    /// Optional display label. Singleton capability providers inherit the nearest ancestor
    /// provider's label when omitted; other Resource actions must declare one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub handler: String,
    #[serde(default)]
    pub applies_to: ResourceActionAppliesToCapability,
    #[serde(default)]
    pub access: ManifestActionAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<ActionRequirements>,
    pub output: ActionOutputCapability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ActionUi>,
}

/// Directory action contributed by a plugin manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryActionCapability {
    pub id: String,
    /// Optional singleton Host capability implemented by this action provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provides: Option<String>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub handler: String,
    #[serde(default)]
    pub applies_to: DirectoryActionAppliesToCapability,
    #[serde(default)]
    pub access: ManifestActionAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<DirectoryActionRequirementsCapability>,
    pub output: ActionOutputCapability,
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
    pub resources: DirectoryResourceAccess,
}

/// Resource data exposed to a Directory Action through its call-scoped directory reference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryResourceAccess {
    #[default]
    None,
    Metadata,
    Content,
}

/// Views and effects an action is allowed to return.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionOutputCapability {
    pub views: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
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
pub struct ResourceActionAppliesToCapability {
    pub kinds: Vec<String>,
    pub mime_types: Vec<String>,
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

/// Optional UI placement hints for host applications.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionUi {
    pub group: Option<String>,
    pub order: Option<i32>,
    pub locations: Vec<String>,
}

/// Content matching declaration used by external resource-kind capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ResourceContentMatcher {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mime_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extensions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceContentMatcherDocument {
    #[serde(default)]
    mime_types: Vec<String>,
    #[serde(default)]
    extensions: Vec<String>,
}

impl<'de> Deserialize<'de> for ResourceContentMatcher {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = ResourceContentMatcherDocument::deserialize(deserializer)?;
        if document
            .mime_types
            .iter()
            .chain(&document.extensions)
            .any(|value| value.trim().is_empty())
        {
            return Err(serde::de::Error::custom(
                "content matcher values must not be empty",
            ));
        }
        Ok(Self::new()
            .with_mime_types(document.mime_types)
            .with_extensions(document.extensions))
    }
}

impl ResourceContentMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mime_types(
        mut self,
        mime_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.mime_types = mime_types
            .into_iter()
            .map(|value| value.into().trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect();
        self
    }

    pub fn with_extensions(
        mut self,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.extensions = extensions
            .into_iter()
            .map(normalize_extension)
            .filter(|value| !value.is_empty())
            .collect();
        self
    }

    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }
}

impl ResourceActionCapability {
    pub fn handler(&self) -> &str {
        &self.handler
    }
}

impl DirectoryActionCapability {
    pub fn handler(&self) -> &str {
        &self.handler
    }
}

fn normalize_extension(value: impl Into<String>) -> String {
    let value = value.into().trim().to_ascii_lowercase();
    if value.is_empty() || value.starts_with('.') {
        value
    } else {
        format!(".{value}")
    }
}
