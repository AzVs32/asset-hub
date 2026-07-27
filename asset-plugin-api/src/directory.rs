use crate::{ActionAccess, PluginDiagnostic, PluginView};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DIRECTORY_HOST_API_VERSION: u32 = 1;
pub const DIRECTORY_LIST_CHILDREN_FN: &str = "asset_hub_directory_list_children";
pub const DIRECTORY_LIST_RESOURCES_FN: &str = "asset_hub_directory_list_resources";

/// Directory action request passed from the host to a plugin handler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginDirectoryActionRequest {
    pub action: String,
    pub access: ActionAccess,
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
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectoryPluginActionOutput {
    #[serde(flatten)]
    pub view: PluginView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<DirectoryActionEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl DirectoryPluginActionOutput {
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

#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
pub mod guest {
    use super::{PluginDirectoryPage, PluginDirectoryResourcePage};
    use extism_pdk::{FnResult, host_fn};

    #[host_fn]
    extern "ExtismHost" {
        fn asset_hub_directory_list_children(request: String) -> String;
        fn asset_hub_directory_list_resources(request: String) -> String;
    }

    pub fn list_children(
        reference: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> FnResult<PluginDirectoryPage> {
        let request = serde_json::json!({"reference": reference, "cursor": cursor, "limit": limit})
            .to_string();
        let response = unsafe { asset_hub_directory_list_children(request) }?;
        Ok(serde_json::from_str(&response)?)
    }

    pub fn list_resources(
        reference: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> FnResult<PluginDirectoryResourcePage> {
        let request = serde_json::json!({"reference": reference, "cursor": cursor, "limit": limit})
            .to_string();
        let response = unsafe { asset_hub_directory_list_resources(request) }?;
        Ok(serde_json::from_str(&response)?)
    }
}
