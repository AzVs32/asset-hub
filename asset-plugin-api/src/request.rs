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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_ref: Option<PluginContentReference>,
}

/// Resource snapshot exposed to plugin handlers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginResource {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub metadata: PluginResourceMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginResourceContent>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

/// Versioned core metadata exposed as part of a resource snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginResourceMetadata {
    pub schema_version: u32,
    pub summary: PluginResourceSummaryMetadata,
}

/// Core summary fields understood by both the host and plugins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginResourceSummaryMetadata {
    pub description: Option<String>,
    pub tags: Vec<String>,
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

/// Non-inline object content supplied to a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginContentReference {
    #[serde(default = "content_abi_version")]
    pub abi_version: u32,
    pub encoding: PluginContentEncoding,
    pub reference: String,
}

fn content_abi_version() -> u32 {
    crate::CONTENT_ABI_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resource_metadata_has_an_explicit_schema() {
        let metadata: PluginResourceMetadata = serde_json::from_value(json!({
            "schema_version": 1,
            "summary": {
                "description": "Document",
                "tags": ["docs"]
            }
        }))
        .unwrap();

        assert_eq!(metadata.schema_version, 1);
        assert_eq!(metadata.summary.description.as_deref(), Some("Document"));
        assert_eq!(metadata.summary.tags, ["docs"]);
    }

    #[test]
    fn resource_metadata_rejects_plugin_defined_fields() {
        let result = serde_json::from_value::<PluginResourceMetadata>(json!({
            "schema_version": 1,
            "summary": {
                "description": null,
                "tags": []
            },
            "plugin_data": {}
        }));

        assert!(result.is_err());
    }
}
