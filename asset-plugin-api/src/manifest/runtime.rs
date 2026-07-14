use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Runtime used to execute plugin actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginRuntime {
    Builtin,
    Extism {
        wasm: PathBuf,
        /// Lowercase SHA-256 digest of the deployed Wasm artifact.
        wasm_sha256: String,
        #[serde(default)]
        wasi: bool,
        plugin_api: String,
    },
}
