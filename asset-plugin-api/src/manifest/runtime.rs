use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Runtime used to execute plugin actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginRuntime {
    Builtin,
    Extism {
        wasm: PathBuf,
        #[serde(default)]
        wasi: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plugin_api: Option<String>,
    },
}
