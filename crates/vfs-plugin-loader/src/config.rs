//! A single host-linked configuration declaration covers every external plugin.
//! Adding a backend changes entries and artifacts, never this Rust type.
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf};
use vfs_plugin_protocol::JsonObject;
use vfs_plugin_wasm::HostGrants;

/// Configure `[[vfs.plugins.entries]]` in a host using ah-config. The enclosing
/// namespace is registered by linking this loader, not by linking plugin crates.
#[derive(ah_config::Config)]
#[config(parent = ah_vfs::config::VfsConfig, key = "plugins", default, deny_unknown_fields)]
pub struct PluginConfig {
    #[config(default = Vec::new())]
    pub entries: Vec<PluginSpec>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PluginSpec {
    /// Optional deployment identity check; the plugin still owns its actual key.
    #[serde(default)]
    pub expected_key: Option<String>,
    #[serde(flatten)]
    pub source: PluginSource,
    /// Plugin-defined values, validated by initialize using its described schema.
    #[serde(default)]
    pub settings: JsonObject,
}
impl fmt::Debug for PluginSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginSpec")
            .field("source", &self.source)
            .field("settings_count", &self.settings.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum PluginSource {
    Grpc {
        endpoint: String,
        /// The loader reads the transport token from this explicit environment name.
        token_env: String,
        #[serde(default)]
        tls: Option<GrpcTls>,
    },
    Wasm {
        module: PathBuf,
        #[serde(default = "wasm_profile")]
        profile: String,
        #[serde(default)]
        grants: HostGrants,
    },
}
impl fmt::Debug for PluginSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Grpc { .. } => "Grpc { .. }",
            Self::Wasm { .. } => "Wasm { .. }",
        })
    }
}

fn wasm_profile() -> String {
    vfs_plugin_protocol::WASM_HOST_PROFILE.into()
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrpcTls {
    #[serde(default)]
    pub ca_certificate: Option<PathBuf>,
    #[serde(default)]
    pub client_certificate: Option<PathBuf>,
    #[serde(default)]
    pub client_key: Option<PathBuf>,
    #[serde(default)]
    pub server_name: Option<String>,
}
