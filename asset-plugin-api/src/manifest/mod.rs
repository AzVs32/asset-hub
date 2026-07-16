mod capabilities;
mod permissions;
mod plugin;
mod runtime;

pub use capabilities::{
    ActionAppliesTo, ActionRequirements, ActionUi, ContentDelivery, ManifestActionAccess,
    PluginCapabilities, ResourceActionCapability, ResourceKindCapability,
};
pub use lock::{PluginManifestLock, PluginRuntimeLock, PluginWebLock};
pub use permissions::{
    FilesystemPermission, NetworkPermission, PluginPermission, PluginPermissions,
    ReadWritePermission,
};
pub use plugin::PluginMetadata;
pub use runtime::PluginRuntime;
pub use web::PluginWeb;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

mod web {
    use serde::{Deserialize, Serialize};
    use std::path::PathBuf;

    /// Browser-facing assets contributed by a plugin.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct PluginWeb {
        pub root: PathBuf,
    }
}

mod lock {
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// Generated integrity data for a plugin package.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct PluginManifestLock {
        pub manifest_version: u32,
        pub plugin_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub runtime: Option<PluginRuntimeLock>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub web: Option<PluginWebLock>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct PluginRuntimeLock {
        pub wasm_sha256: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct PluginWebLock {
        pub integrity: BTreeMap<PathBuf, String>,
    }
}

/// Current manifest schema version. V2 remains loadable during the V3 compatibility window.
pub const MANIFEST_VERSION: u32 = 3;
pub const MIN_MANIFEST_VERSION: u32 = 2;
pub const PLUGIN_API_VERSION: &str = "asset-hub.plugin-api@0.2";
pub const MIN_PLUGIN_API_VERSION: &str = "asset-hub.plugin-api@0.1";
/// Editable Manifest V3 draft copied by `asset-plugin gen manifest`.
pub const MANIFEST_TEMPLATE: &str = include_str!("../../templates/manifest.json");
pub const MANIFEST_SCHEMA: &str = include_str!("../../schema/plugin-manifest-v3.schema.json");

/// Complete plugin manifest document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "$schema")]
    pub schema: Option<String>,
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
        if !(MIN_MANIFEST_VERSION..=MANIFEST_VERSION).contains(&self.manifest_version) {
            return Err(format!(
                "unsupported manifest_version `{}`; supported range is {MIN_MANIFEST_VERSION}..={MANIFEST_VERSION}",
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
                wasm, plugin_api, ..
            } => {
                if wasm.as_os_str().is_empty() {
                    return Err("runtime.wasm must not be empty".to_string());
                }
                validate_relative_path("runtime.wasm", wasm)?;
                validate_plugin_api_version(plugin_api)?;
            }
        }
        if let Some(web) = &self.web {
            validate_relative_path("web.root", &web.root)?;
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

/// Returns whether a guest ABI is supported by this host. Pre-1.0 minor versions are treated as
/// explicit protocol levels, so compatibility is an allow-list rather than a loose semver match.
pub fn is_plugin_api_compatible(value: &str) -> bool {
    matches!(value, MIN_PLUGIN_API_VERSION | PLUGIN_API_VERSION)
}

fn validate_plugin_api_version(value: &str) -> Result<(), String> {
    if is_plugin_api_compatible(value) {
        Ok(())
    } else {
        Err(format!(
            "unsupported runtime.plugin_api `{value}`; supported versions are `{MIN_PLUGIN_API_VERSION}` and `{PLUGIN_API_VERSION}`"
        ))
    }
}

impl PluginManifestLock {
    pub fn validate_for(&self, manifest: &PluginManifest) -> Result<(), String> {
        if self.manifest_version != manifest.manifest_version {
            return Err(format!(
                "manifest.lock.json manifest_version `{}` does not match manifest `{}`",
                self.manifest_version, manifest.manifest_version
            ));
        }
        if self.plugin_id != manifest.plugin.id {
            return Err(format!(
                "manifest.lock.json plugin_id `{}` does not match manifest plugin.id `{}`",
                self.plugin_id, manifest.plugin.id
            ));
        }
        match &manifest.runtime {
            PluginRuntime::Builtin => {
                if self.runtime.is_some() {
                    return Err(
                        "manifest.lock.json runtime is only valid for extism plugins".to_string(),
                    );
                }
            }
            PluginRuntime::Extism { .. } => {
                let Some(runtime) = &self.runtime else {
                    return Err("manifest.lock.json runtime.wasm_sha256 is required".to_string());
                };
                validate_digest(
                    "manifest.lock.json runtime.wasm_sha256",
                    &runtime.wasm_sha256,
                )?;
            }
        }
        match (&manifest.web, &self.web) {
            (None, Some(_)) => {
                return Err(
                    "manifest.lock.json web is only valid when manifest.web is present".to_string(),
                );
            }
            (Some(_), None) => {
                return Err("manifest.lock.json web.integrity is required".to_string());
            }
            (Some(_), Some(web)) => {
                if web.integrity.is_empty() {
                    return Err("manifest.lock.json web.integrity must not be empty".to_string());
                }
                for (path, digest) in &web.integrity {
                    validate_relative_path("manifest.lock.json web.integrity path", path)?;
                    validate_digest(
                        &format!("manifest.lock.json web.integrity[`{}`]", path.display()),
                        digest,
                    )?;
                }
            }
            (None, None) => {}
        }
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
        validate_id("capabilities.kinds[].kind", &kind.kind, &[':', '-', '_'])?;
        if kind
            .parent
            .as_ref()
            .is_some_and(|parent| parent.trim().is_empty())
        {
            return Err("capabilities.kinds[].parent must not be empty".to_string());
        }
    }
    for action in &capabilities.resource_actions {
        validate_id(
            "capabilities.actions[].id",
            &action.id,
            &['.', ':', '-', '_'],
        )?;
        if !action_ids.insert(action.id.as_str()) {
            return Err(format!("duplicate resource action `{}`", action.id));
        }
        if action.label.trim().is_empty() {
            return Err(format!(
                "capabilities.actions[`{}`].label must not be empty",
                action.id
            ));
        }
        if !manifest.permissions.resource_metadata_read() {
            return Err(format!(
                "capabilities.actions[`{}`] lacks resource.metadata.read permission",
                action.id
            ));
        }
        validate_id("handler", &action.handler, &['.', '-', '_'])?;
        if action.views.is_empty() {
            return Err(format!(
                "capabilities.actions[`{}`].views must not be empty",
                action.id
            ));
        }
        for view in &action.views {
            if !SUPPORTED_VIEWS.contains(&view.as_str()) {
                return Err(format!(
                    "capabilities.actions[`{}`] declares unsupported view `{view}`",
                    action.id
                ));
            }
        }
        if action.views.iter().any(|view| view == "plugin_frame") && manifest.web.is_none() {
            return Err(format!(
                "capabilities.actions[`{}`] returns plugin_frame but plugin.web is missing",
                action.id
            ));
        }
        if action
            .requires
            .as_ref()
            .is_some_and(|requires| requires.content)
            && !manifest.permissions.content_read()
        {
            return Err(format!(
                "capabilities.actions[`{}`] requires content without content.read permission",
                action.id
            ));
        }
        if matches!(action.access, ManifestActionAccess::Write)
            && !manifest.permissions.resource_metadata_write()
            && !manifest.permissions.content_replace()
            && !manifest.permissions.derived_asset_write()
        {
            return Err(format!(
                "capabilities.actions[`{}`] is writable without a write permission",
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

fn validate_digest(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be a lowercase SHA-256 digest"));
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
    fn embedded_manifest_template_is_a_v3_draft_without_generated_integrity() {
        let document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();

        assert_eq!(document["manifest_version"], MANIFEST_VERSION);
        assert!(document["runtime"].get("plugin_api").is_none());
        assert!(document["runtime"].get("wasm_sha256").is_none());
        assert!(document.get("web").is_none());
    }

    #[test]
    fn embedded_json_schema_is_draft_2020_12() {
        let schema: serde_json::Value = serde_json::from_str(MANIFEST_SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(
            schema["properties"]["manifest_version"]["const"],
            MANIFEST_VERSION
        );
    }

    #[test]
    fn compatibility_window_accepts_v2_api_01_and_rejects_unknown_versions() {
        let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
        document.as_object_mut().unwrap().remove("$schema");
        document["manifest_version"] = serde_json::json!(2);
        document["runtime"]["plugin_api"] = serde_json::json!(MIN_PLUGIN_API_VERSION);
        let capabilities = document["capabilities"].as_object_mut().unwrap();
        let actions = capabilities.remove("actions").unwrap();
        capabilities.insert("resource_actions".to_string(), actions);
        document["permissions"] = serde_json::json!({
            "resource": {"read": true, "write": false},
            "content": {"read": false, "write": false},
            "network": false,
            "filesystem": false
        });
        let manifest: PluginManifest = serde_json::from_value(document.clone()).unwrap();
        manifest.validate().unwrap();

        document["manifest_version"] = serde_json::json!(1);
        assert!(
            serde_json::from_value::<PluginManifest>(document.clone())
                .unwrap()
                .validate()
                .is_err()
        );
        document["manifest_version"] = serde_json::json!(2);
        document["runtime"]["plugin_api"] = serde_json::json!("asset-hub.plugin-api@0.3");
        assert!(
            serde_json::from_value::<PluginManifest>(document)
                .unwrap()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn manifest_rejects_unknown_fields_at_every_level() {
        let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
        document["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<PluginManifest>(document).is_err());

        let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
        document["capabilities"]["actions"][0]["applies_to"]["typo"] = serde_json::json!([]);
        assert!(serde_json::from_value::<PluginManifest>(document).is_err());

        let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
        document["runtime"]["wais"] = serde_json::json!(false);
        assert!(serde_json::from_value::<PluginManifest>(document).is_err());
    }
}
