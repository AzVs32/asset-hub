//! Manifest 中的插件身份和注册描述。
//!
//! `PluginDescriptor` 是 manifest 文档的一部分，描述稳定插件 ID 及面向注册表的元数据。

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
