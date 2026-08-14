//! Directory Action 的 JSON 调用协议。
//!
//! 本模块只描述目录快照、分页结果和插件可声明的目录副作用。Directory Host ABI
//! 常量与 guest helper 定义在 [`crate::abi::directory`]。

use crate::protocol::{
    PluginActionAccess, PluginContentReference, PluginDiagnostic, PluginResourceContent, PluginView,
};
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
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub view: Option<PluginView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<DirectoryActionEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl PluginDirectoryActionOutput {
    pub fn new(view: PluginView) -> Self {
        Self {
            view: Some(view),
            effects: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn without_view() -> Self {
        Self {
            view: None,
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
    Delete,
}

impl DirectoryActionEffect {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Update(_) => "update",
            Self::CreateChild(_) => "create_child",
            Self::Delete => "delete",
        }
    }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginDirectoryResourcePage {
    pub items: Vec<PluginDirectoryResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginDirectoryResource {
    /// Stable Resource identity within the current workspace.
    pub id: String,
    pub name: String,
    pub kind: String,
    pub revision: u64,
    /// Content metadata exposed by both `metadata` and `content` resource access modes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginResourceContent>,
    /// Opaque, call-scoped reference issued only for readable content in `content` mode.
    ///
    /// It is consumed by the standard content Host ABI and is never a filesystem path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_ref: Option<PluginContentReference>,
}

#[cfg(test)]
mod tests {
    use super::{DirectoryActionEffect, PluginDirectoryActionOutput, PluginDirectoryResourcePage};

    #[test]
    fn delete_effect_is_bound_to_the_current_directory() {
        let mut output = PluginDirectoryActionOutput::without_view();
        output.effects.push(DirectoryActionEffect::Delete);

        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({"effects": [{"type": "delete"}]})
        );
    }

    #[test]
    fn directory_resource_content_uses_the_standard_call_scoped_handle_shape() {
        let page: PluginDirectoryResourcePage = serde_json::from_value(serde_json::json!({
            "items": [{
                "id": "01900000-0000-7000-8000-000000000001",
                "name": "README.md",
                "kind": "plugin:resource:games:readme",
                "revision": 2,
                "content": {
                    "size": 12,
                    "mime_type": "text/markdown",
                    "verification_status": "verified"
                },
                "content_ref": {
                    "encoding": "handle",
                    "reference": "content:reference:call-scoped"
                }
            }]
        }))
        .unwrap();

        let resource = &page.items[0];
        assert_eq!(resource.name, "README.md");
        assert_eq!(resource.kind, "plugin:resource:games:readme");
        assert_eq!(resource.content.as_ref().unwrap().size, 12);
        assert_eq!(
            resource.content_ref.as_ref().unwrap().reference,
            "content:reference:call-scoped"
        );
    }
}
