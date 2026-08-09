//! Directory Action 的 JSON 调用协议。
//!
//! 本模块只描述目录快照、分页结果和插件可声明的目录副作用。Directory Host ABI
//! 常量与 guest helper 定义在 [`crate::abi::directory`]。

use crate::protocol::{PluginActionAccess, PluginDiagnostic, PluginView};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Directory action request passed from the host to a plugin handler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginDirectoryActionRequest {
    pub action: String,
    pub access: PluginActionAccess,
    #[serde(default)]
    pub input: Value,
    pub directory: PluginDirectory,
    /// Opaque, call-scoped reference accepted by directory Host APIs.
    pub directory_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDirectory {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub path: String,
    pub name: String,
    pub kind: String,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginDirectoryActionOutput {
    #[serde(flatten)]
    pub view: PluginView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<DirectoryActionEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl PluginDirectoryActionOutput {
    pub fn new(view: PluginView) -> Self {
        Self {
            view,
            effects: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DirectoryActionEffect {
    Update(UpdateDirectoryEffect),
    CreateChild(CreateChildDirectoryEffect),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateDirectoryEffect {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateChildDirectoryEffect {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDirectoryPage {
    pub items: Vec<PluginDirectoryChild>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDirectoryChild {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDirectoryResourcePage {
    pub items: Vec<PluginDirectoryResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDirectoryResource {
    pub id: String,
    pub name: String,
    pub kind: String,
}
