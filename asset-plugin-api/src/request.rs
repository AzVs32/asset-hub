use crate::ResourceActionAccess;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Action request passed from host to a plugin handler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginActionRequest {
    pub action: String,
    pub access: ResourceActionAccess,
    #[serde(default)]
    pub input: Value,
    pub resource: PluginResource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginContentBytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_ref: Option<PluginContentReference>,
}

/// Resource snapshot exposed to plugin handlers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginResource {
    pub id: String,
    pub directory: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginResourceContent>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

/// Resource content reference exposed to plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginResourceContent {
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub checksum: PluginChecksum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginChecksum {
    pub kind: String,
    pub value: String,
}

/// Inline object content supplied to a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginContentBytes {
    pub encoding: PluginInlineContentEncoding,
    pub data: String,
}

/// Encoding accepted for content embedded directly in an action request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginInlineContentEncoding {
    Base64,
}

/// Non-inline object content supplied to a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginContentReference {
    #[serde(default = "content_abi_version")]
    pub abi_version: u32,
    pub encoding: PluginContentReferenceEncoding,
    pub reference: String,
}

/// Encoding used by an opaque, call-scoped host content reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginContentReferenceEncoding {
    Handle,
}

fn content_abi_version() -> u32 {
    crate::CONTENT_ABI_VERSION
}
