//! Manifest 中声明的插件执行运行时。
//!
//! 该模块只描述运行时选择及其包内入口，不负责加载 Wasm、创建执行器或实施资源限制。

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
        plugin_api: String,
    },
}

impl PluginRuntime {
    pub fn plugin_api(&self) -> Option<&str> {
        match self {
            Self::Builtin => None,
            Self::Extism { plugin_api, .. } => Some(plugin_api),
        }
    }
}
