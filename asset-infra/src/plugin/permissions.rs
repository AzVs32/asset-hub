use asset_core::CoreError;
use asset_core::domain::{ActionAccess, ResourceActionContentDelivery};
use asset_core::port::ResourceActionRequest;
use asset_plugin_api::manifest::{PluginPermission, PluginPermissions};
use asset_plugin_api::protocol::{PluginDiagnostic, PluginDiagnosticSeverity};
use extism::{Manifest, Wasm};
use std::path::{Component, Path};

use super::executor::ActionBinding;
use super::policy::PluginExecutionPolicy;
use crate::config::PluginPermissionGrants;

pub(super) fn validate_external_permissions(
    plugin_id: &str,
    permissions: &PluginPermissions,
    grants: &PluginPermissionGrants,
) -> Result<(), CoreError> {
    if permissions.resource_delete() && !grants.resource_delete {
        return Err(CoreError::configuration(format!(
            "plugin `{plugin_id}` requests resource.delete without a matching host grant"
        )));
    }
    if permissions.directory_delete() && !grants.directory_delete {
        return Err(CoreError::configuration(format!(
            "plugin `{plugin_id}` requests directory.delete without a matching host grant"
        )));
    }
    for host in permissions.network.hosts() {
        if host.contains('*') || !grants.network_hosts.iter().any(|grant| grant == host) {
            return Err(CoreError::configuration(format!(
                "plugin `{plugin_id}` requests network host `{host}` without a matching host grant"
            )));
        }
    }
    for path in permissions.filesystem.read_paths() {
        let requested = validated_permission_path(plugin_id, "filesystem.read", path)?;
        let granted = grants
            .filesystem_read
            .iter()
            .chain(&grants.filesystem_write)
            .any(|grant| requested.starts_with(grant));
        if !granted {
            return Err(CoreError::configuration(format!(
                "plugin `{plugin_id}` requests filesystem read path `{path}` without a matching host grant"
            )));
        }
    }
    for path in permissions.filesystem.write_paths() {
        let requested = validated_permission_path(plugin_id, "filesystem.write", path)?;
        if !grants
            .filesystem_write
            .iter()
            .any(|grant| requested.starts_with(grant))
        {
            return Err(CoreError::configuration(format!(
                "plugin `{plugin_id}` requests filesystem write path `{path}` without a matching host grant"
            )));
        }
    }
    Ok(())
}

pub(super) fn validated_permission_path<'a>(
    plugin_id: &str,
    field: &str,
    value: &'a str,
) -> Result<&'a Path, CoreError> {
    let path = Path::new(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(CoreError::configuration(format!(
            "plugin `{plugin_id}` {field} path `{value}` must be absolute and canonical"
        )));
    }
    Ok(path)
}

pub(super) fn verify_permissions(
    binding: &ActionBinding,
    request: &ResourceActionRequest,
) -> Result<(), CoreError> {
    if !binding.permissions.allows(PluginPermission::ResourceRead) {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks resource.read permission",
            binding.plugin_id, binding.action
        )));
    }
    if matches!(request.access(), ActionAccess::Write)
        && !binding.permissions.resource_content_replace()
        && !binding.permissions.resource_delete()
    {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks a write permission",
            binding.plugin_id, binding.action
        )));
    }
    if !matches!(
        request.content_delivery(),
        ResourceActionContentDelivery::Auto
    ) && !binding
        .permissions
        .allows(PluginPermission::ResourceContentRead)
    {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks resource.content.read permission",
            binding.plugin_id, binding.action
        )));
    }
    Ok(())
}

pub(super) fn verify_content_budget(
    binding: &ActionBinding,
    request: &ResourceActionRequest,
    policy: &PluginExecutionPolicy,
) -> Result<(), CoreError> {
    if matches!(
        request.content_delivery(),
        ResourceActionContentDelivery::Auto
    ) {
        return Ok(());
    }
    let declared_size = request
        .resource()
        .content()
        .map(|content| content.size())
        .unwrap_or_default();
    let loaded_size = request
        .content()
        .map(|content| content.len() as u64)
        .unwrap_or_default();
    let size = declared_size.max(loaded_size);
    if size > policy.max_content_bytes() {
        return Err(CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            host_diagnostic(
                asset_plugin_api::protocol::diagnostic::codes::CONTENT_LIMIT_EXCEEDED,
                format!(
                    "resource content is {size} bytes, plugin limit is {}",
                    policy.max_content_bytes()
                ),
            ),
        ));
    }
    Ok(())
}

pub(super) fn host_diagnostic(code: &str, message: impl Into<String>) -> PluginDiagnostic {
    PluginDiagnostic {
        code: code.to_string(),
        message: message.into(),
        severity: PluginDiagnosticSeverity::Error,
        retryable: false,
        details: None,
    }
}

pub(super) fn manifest_for_plugin(
    wasm: &[u8],
    permissions: &PluginPermissions,
    policy: &PluginExecutionPolicy,
) -> Manifest {
    let mut manifest = Manifest::new([Wasm::data(wasm.to_vec())])
        .with_memory_max(policy.memory_max_pages())
        .with_timeout(std::time::Duration::from_secs(policy.timeout_seconds()));

    if permissions.network.enabled() {
        manifest = manifest.with_allowed_hosts(permissions.network.hosts().iter().cloned());
    }

    for path in permissions.filesystem.read_paths() {
        manifest = manifest.with_allowed_path(format!("ro:{path}"), path);
    }
    for path in permissions.filesystem.write_paths() {
        manifest = manifest.with_allowed_path(path.clone(), path);
    }

    manifest
}
