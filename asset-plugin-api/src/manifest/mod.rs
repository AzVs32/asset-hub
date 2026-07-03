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

mod web {
    use serde::{Deserialize, Serialize};
    use std::path::PathBuf;

    /// Browser-facing assets contributed by a plugin.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PluginWeb {
        pub root: PathBuf,
    }
}

/// Current manifest schema version.
pub const MANIFEST_VERSION: u32 = 1;
pub const PLUGIN_API_VERSION: &str = "asset-hub.plugin-api@0.1";

/// Complete plugin manifest document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        if self.plugin.id.trim().is_empty() {
            return Err("plugin.id must not be empty".to_string());
        }
        if let PluginRuntime::Extism {
            plugin_api: Some(plugin_api),
            ..
        } = &self.runtime
            && plugin_api != PLUGIN_API_VERSION
        {
            return Err(format!(
                "unsupported runtime.plugin_api `{plugin_api}`, expected `{PLUGIN_API_VERSION}`"
            ));
        }
        validate_capabilities(&self.capabilities)?;
        Ok(())
    }
}

fn validate_capabilities(capabilities: &PluginCapabilities) -> Result<(), String> {
    for kind in &capabilities.resource_kinds {
        if kind.kind.trim().is_empty() {
            return Err("capabilities.resource_kinds[].kind must not be empty".to_string());
        }
    }
    for action in &capabilities.resource_actions {
        if action.id.trim().is_empty() {
            return Err("capabilities.resource_actions[].id must not be empty".to_string());
        }
        if action.label.trim().is_empty() {
            return Err(format!(
                "capabilities.resource_actions[`{}`].label must not be empty",
                action.id
            ));
        }
        if matches!(
            action.executor,
            Some(ActionExecutor::Builtin { ref handler } | ActionExecutor::Plugin { ref handler })
                if handler.trim().is_empty()
        ) {
            return Err(format!(
                "capabilities.resource_actions[`{}`].executor.handler must not be empty",
                action.id
            ));
        }
    }
    Ok(())
}
