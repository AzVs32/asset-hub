use crate::{PluginContentEncoding, ResourceActionAccess};
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
}

/// Resource snapshot exposed to plugin handlers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginResource {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginResourceContent>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

/// Resource content reference exposed to plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginResourceContent {
    pub key: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub checksum: Vec<PluginChecksum>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginChecksum {
    pub kind: String,
    pub value: String,
}

/// Inline object content supplied to a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginContentBytes {
    pub encoding: PluginContentEncoding,
    pub data: String,
}
