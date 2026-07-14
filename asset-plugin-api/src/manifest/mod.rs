mod capabilities;
mod permissions;
mod plugin;
mod runtime;

pub use capabilities::{
    ActionAppliesTo, ActionExecutor, ActionOutputContract, ActionRequirements, ActionUi,
    ContentDelivery, ManifestActionAccess, PluginCapabilities, ResourceActionCapability,
    ResourceKindCapability,
};
pub use permissions::{
    FilesystemPermission, NetworkPermission, PluginPermissions, ReadWritePermission,
};
pub use plugin::PluginMetadata;
pub use runtime::PluginRuntime;
pub use web::PluginWeb;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

mod web {
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// Browser-facing assets contributed by a plugin.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct PluginWeb {
        pub root: PathBuf,
        pub integrity: BTreeMap<PathBuf, String>,
    }
}

/// Current manifest schema version.
pub const MANIFEST_VERSION: u32 = 2;
pub const PLUGIN_API_VERSION: &str = "asset-hub.plugin-api@0.1";
/// Editable Manifest V2 draft copied by `asset-plugin gen manifest`.
pub const MANIFEST_TEMPLATE: &str = include_str!("../../templates/manifest.json");

/// Complete plugin manifest document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub manifest_version: u32,
    pub plugin: PluginMetadata,
    pub runtime: PluginRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<PluginWeb>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    pub permissions: PluginPermissions,
}

impl PluginManifest {
    pub fn plugin_id(&self) -> &str {
        &self.plugin.id
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.manifest_version != MANIFEST_VERSION {
            return Err(format!(
                "unsupported manifest_version `{}`",
                self.manifest_version
            ));
        }
        validate_id("plugin.id", &self.plugin.id, &['.', '-', '_'])?;
        if self.plugin.name.trim().is_empty() || self.plugin.version.trim().is_empty() {
            return Err("plugin.name and plugin.version must not be empty".to_string());
        }
        match &self.runtime {
            PluginRuntime::Builtin => {}
            PluginRuntime::Extism {
                wasm,
                wasm_sha256,
                plugin_api,
                ..
            } => {
                if wasm.as_os_str().is_empty() {
                    return Err("runtime.wasm must not be empty".to_string());
                }
                validate_relative_path("runtime.wasm", wasm)?;
                if wasm_sha256.len() != 64
                    || !wasm_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(
                        "runtime.wasm_sha256 must be a lowercase SHA-256 digest".to_string()
                    );
                }
                if plugin_api != PLUGIN_API_VERSION {
                    return Err(format!(
                        "unsupported runtime.plugin_api `{plugin_api}`, expected `{PLUGIN_API_VERSION}`"
                    ));
                }
            }
        }
        if let Some(web) = &self.web {
            validate_relative_path("web.root", &web.root)?;
            if web.integrity.is_empty() {
                return Err("web.integrity must not be empty".to_string());
            }
            for (path, digest) in &web.integrity {
                validate_relative_path("web.integrity path", path)?;
                if digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(format!(
                        "web.integrity[`{}`] must be a lowercase SHA-256 digest",
                        path.display()
                    ));
                }
            }
        }
        if self.permissions.network.enabled() && !self.permissions.network.has_scope() {
            return Err("permissions.network must declare an explicit host scope".to_string());
        }
        if self.permissions.filesystem.enabled() && !self.permissions.filesystem.has_scope() {
            return Err("permissions.filesystem must declare explicit path scopes".to_string());
        }
        validate_capabilities(self)?;
        Ok(())
    }
}

const SUPPORTED_VIEWS: &[&str] = &[
    "text",
    "markdown",
    "html",
    "plugin_frame",
    "json",
    "media",
    "binary_url",
    "table",
    "form",
];

fn validate_capabilities(manifest: &PluginManifest) -> Result<(), String> {
    let capabilities = &manifest.capabilities;
    let mut action_ids = HashSet::new();
    for kind in &capabilities.resource_kinds {
        validate_id(
            "capabilities.resource_kinds[].kind",
            &kind.kind,
            &[':', '-', '_'],
        )?;
        if kind
            .parent
            .as_ref()
            .is_some_and(|parent| parent.trim().is_empty())
        {
            return Err("capabilities.resource_kinds[].parent must not be empty".to_string());
        }
    }
    for action in &capabilities.resource_actions {
        validate_id(
            "capabilities.resource_actions[].id",
            &action.id,
            &['.', ':', '-', '_'],
        )?;
        if !action_ids.insert(action.id.as_str()) {
            return Err(format!("duplicate resource action `{}`", action.id));
        }
        if action.label.trim().is_empty() {
            return Err(format!(
                "capabilities.resource_actions[`{}`].label must not be empty",
                action.id
            ));
        }
        if !manifest.permissions.resource.read {
            return Err(format!(
                "capabilities.resource_actions[`{}`] lacks resource.read permission",
                action.id
            ));
        }
        let Some(executor) = &action.executor else {
            return Err(format!(
                "capabilities.resource_actions[`{}`].executor is required",
                action.id
            ));
        };
        let handler = match executor {
            ActionExecutor::Builtin { handler } | ActionExecutor::Plugin { handler } => handler,
        };
        validate_id("executor.handler", handler, &['.', '-', '_'])?;
        let runtime_matches = matches!(
            (&manifest.runtime, executor),
            (PluginRuntime::Builtin, ActionExecutor::Builtin { .. })
                | (PluginRuntime::Extism { .. }, ActionExecutor::Plugin { .. })
        );
        if !runtime_matches {
            return Err(format!(
                "capabilities.resource_actions[`{}`].executor does not match runtime.type",
                action.id
            ));
        }
        let output = action.output.as_ref().ok_or_else(|| {
            format!(
                "capabilities.resource_actions[`{}`].output is required",
                action.id
            )
        })?;
        if output.view.is_empty() {
            return Err(format!(
                "capabilities.resource_actions[`{}`].output.view must not be empty",
                action.id
            ));
        }
        for view in &output.view {
            if !SUPPORTED_VIEWS.contains(&view.as_str()) {
                return Err(format!(
                    "capabilities.resource_actions[`{}`] declares unsupported view `{view}`",
                    action.id
                ));
            }
        }
        if output.view.iter().any(|view| view == "plugin_frame") && manifest.web.is_none() {
            return Err(format!(
                "capabilities.resource_actions[`{}`] returns plugin_frame but plugin.web is missing",
                action.id
            ));
        }
        if action
            .requires
            .as_ref()
            .is_some_and(|requires| requires.content)
            && !manifest.permissions.content.read
        {
            return Err(format!(
                "capabilities.resource_actions[`{}`] requires content without content.read permission",
                action.id
            ));
        }
        if matches!(action.access, ManifestActionAccess::Write)
            && (!manifest.permissions.resource.write || !manifest.permissions.content.write)
        {
            return Err(format!(
                "capabilities.resource_actions[`{}`] is writable without resource.write and content.write permissions",
                action.id
            ));
        }
    }
    Ok(())
}

fn validate_relative_path(field: &str, path: &std::path::Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("{field} must be a safe relative path"));
    }
    Ok(())
}

fn validate_id(field: &str, value: &str, punctuation: &[char]) -> Result<(), String> {
    if value.is_empty() || value.trim() != value {
        return Err(format!("{field} must be non-empty and canonical"));
    }
    if !value.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || punctuation.contains(&character)
    }) {
        return Err(format!("{field} contains invalid characters: `{value}`"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_template_is_a_v2_draft_without_generated_integrity() {
        let document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();

        assert_eq!(document["manifest_version"], MANIFEST_VERSION);
        assert_eq!(document["runtime"]["plugin_api"], PLUGIN_API_VERSION);
        assert!(document["runtime"].get("wasm_sha256").is_none());
        assert!(document.get("web").is_none());
    }

    #[test]
    fn manifest_rejects_unknown_fields_at_every_level() {
        let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
        document["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<PluginManifest>(document).is_err());

        let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
        document["capabilities"]["resource_actions"][0]["applies_to"]["typo"] =
            serde_json::json!([]);
        assert!(serde_json::from_value::<PluginManifest>(document).is_err());

        let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
        document["runtime"]["wais"] = serde_json::json!(false);
        assert!(serde_json::from_value::<PluginManifest>(document).is_err());
    }
}
