use asset_core::CoreError;
use asset_plugin_api::PluginExecutionPolicy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{
    DEFAULT_PLUGIN_MAX_CONCURRENT_CALLS, DEFAULT_PLUGIN_MAX_CONTENT_BYTES,
    DEFAULT_PLUGIN_MAX_CONTENT_READ_BYTES, DEFAULT_PLUGIN_MAX_INLINE_CONTENT_BYTES,
    DEFAULT_PLUGIN_MAX_INPUT_BYTES, DEFAULT_PLUGIN_MAX_OUTPUT_BYTES,
    DEFAULT_PLUGIN_MEMORY_MAX_PAGES, DEFAULT_PLUGIN_TIMEOUT_SECONDS, normalize_permission_grant,
};

/// 插件宿主策略。Manifest 只能请求权限，最终授权必须同时出现在这里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginHostConfig {
    pub max_content_bytes: u64,
    pub max_inline_content_bytes: u64,
    pub max_content_read_bytes: u64,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_concurrent_calls: usize,
    pub memory_max_pages: u32,
    pub timeout_seconds: u64,
    pub grants: PluginPermissionGrants,
}

impl Default for PluginHostConfig {
    fn default() -> Self {
        Self {
            max_content_bytes: DEFAULT_PLUGIN_MAX_CONTENT_BYTES,
            max_inline_content_bytes: DEFAULT_PLUGIN_MAX_INLINE_CONTENT_BYTES,
            max_content_read_bytes: DEFAULT_PLUGIN_MAX_CONTENT_READ_BYTES,
            max_input_bytes: DEFAULT_PLUGIN_MAX_INPUT_BYTES,
            max_output_bytes: DEFAULT_PLUGIN_MAX_OUTPUT_BYTES,
            max_concurrent_calls: DEFAULT_PLUGIN_MAX_CONCURRENT_CALLS,
            memory_max_pages: DEFAULT_PLUGIN_MEMORY_MAX_PAGES,
            timeout_seconds: DEFAULT_PLUGIN_TIMEOUT_SECONDS,
            grants: PluginPermissionGrants::default(),
        }
    }
}

impl PluginHostConfig {
    pub fn execution_policy(&self) -> Result<PluginExecutionPolicy, CoreError> {
        PluginExecutionPolicy::new(
            self.max_content_bytes,
            self.max_inline_content_bytes,
            self.max_content_read_bytes,
            self.max_input_bytes,
            self.max_output_bytes,
            self.max_concurrent_calls,
            self.memory_max_pages,
            self.timeout_seconds,
        )
        .map_err(|error| CoreError::configuration(error.to_string()))
    }

    pub(super) fn normalize_and_validate(&mut self) -> Result<(), CoreError> {
        self.execution_policy()?;
        for host in &self.grants.network_hosts {
            if host.is_empty() || host.trim() != host || host.contains('*') {
                return Err(CoreError::configuration(format!(
                    "plugin.grants.network_hosts contains invalid host `{host}`"
                )));
            }
        }
        self.grants.filesystem_read = self
            .grants
            .filesystem_read
            .iter()
            .map(|path| normalize_permission_grant(path))
            .collect::<Result<_, _>>()?;
        self.grants.filesystem_write = self
            .grants
            .filesystem_write
            .iter()
            .map(|path| normalize_permission_grant(path))
            .collect::<Result<_, _>>()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginPermissionGrants {
    pub network_hosts: Vec<String>,
    pub filesystem_read: Vec<PathBuf>,
    pub filesystem_write: Vec<PathBuf>,
}
