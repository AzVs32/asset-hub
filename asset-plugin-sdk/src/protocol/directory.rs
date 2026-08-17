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
    CreateTree(CreateDirectoryTreeEffect),
    Delete,
}

impl DirectoryActionEffect {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Update(_) => "update",
            Self::CreateChild(_) => "create_child",
            Self::CreateTree(_) => "create_tree",
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

/// Creates a bounded set of relative directories and resources below the current Directory.
///
/// The Host validates all paths and kinds, applies user authorization, and compensates newly
/// created entries when a later entry fails. File roles and required structure remain plugin
/// policy rather than fields in the Manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDirectoryTreeEffect {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directories: Vec<CreateTreeDirectory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<CreateTreeResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTreeDirectory {
    /// Canonical, non-empty path relative to the action's current Directory.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTreeResource {
    /// Canonical Directory path relative to the action's current Directory; empty means current.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub directory: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub encoding: CreateTreeResourceEncoding,
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreateTreeResourceEncoding {
    Base64,
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
    use super::{
        CreateDirectoryTreeEffect, CreateTreeDirectory, CreateTreeResource,
        CreateTreeResourceEncoding, DirectoryActionEffect, PluginDirectoryActionOutput,
        PluginDirectoryResourcePage,
    };

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

    #[test]
    fn create_tree_uses_relative_directory_and_resource_entries() {
        let mut output = PluginDirectoryActionOutput::without_view();
        output.effects.push(DirectoryActionEffect::CreateTree(
            CreateDirectoryTreeEffect {
                directories: vec![CreateTreeDirectory {
                    path: "game/public".to_string(),
                    kind: Some("core:directory".to_string()),
                }],
                resources: vec![CreateTreeResource {
                    directory: "game".to_string(),
                    name: "README.md".to_string(),
                    kind: Some("example:document".to_string()),
                    mime_type: Some("text/markdown".to_string()),
                    encoding: CreateTreeResourceEncoding::Base64,
                    data: "IyBHYW1l".to_string(),
                }],
            },
        ));

        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({"effects": [{
                "type": "create_tree",
                "directories": [{"path": "game/public", "kind": "core:directory"}],
                "resources": [{
                    "directory": "game",
                    "name": "README.md",
                    "kind": "example:document",
                    "mime_type": "text/markdown",
                    "encoding": "base64",
                    "data": "IyBHYW1l"
                }]
            }]})
        );
    }
}
