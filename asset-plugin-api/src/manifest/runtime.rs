//! Manifest 中声明的插件执行运行时。
//!
//! Extism 插件的包内入口固定为 `plugin.wasm`；该模块只描述运行时选择和协议版本。

use serde::{Deserialize, Serialize};

/// External runtime used to execute plugin actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginRuntime {
    Extism {
        #[serde(default)]
        wasi: bool,
        plugin_api: String,
    },
}

impl PluginRuntime {
    pub fn plugin_api(&self) -> &str {
        match self {
            Self::Extism { plugin_api, .. } => plugin_api,
        }
    }
}
