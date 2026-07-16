use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Runtime used to execute plugin actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginRuntime {
    Builtin,
    Extism {
        wasm: PathBuf,
        #[serde(default)]
        wasi: bool,
        #[serde(default = "default_plugin_api_version")]
        plugin_api: String,
    },
}

fn default_plugin_api_version() -> String {
    super::PLUGIN_API_VERSION.to_string()
}

impl PluginRuntime {
    pub fn plugin_api(&self) -> Option<&str> {
        match self {
            Self::Builtin => None,
            Self::Extism { plugin_api, .. } => Some(plugin_api),
        }
    }
}
