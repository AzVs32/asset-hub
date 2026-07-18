use serde::{Deserialize, Serialize};

/// Stable identity and registry description for a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
