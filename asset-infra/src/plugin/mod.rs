use asset_core::CoreError;
use asset_core::domain::{ChecksumKind, ResourceStatus, StorageKey};
use asset_core::port::{
    BlobStorage, ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest,
    ResourceKindRegistry,
};
use asset_plugin_api::{
    PluginActionOutput, PluginActionRequest, PluginChecksum, PluginContentBytes,
    PluginContentEncoding, PluginContentReference, PluginPermissions, PluginResource,
    PluginResourceContent, PluginResourceMetadata, PluginResourceSummaryMetadata, PluginRuntime,
    PluginView, ResourceActionCapability,
};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use extism::{CompiledPlugin, Function, Manifest, PTR, Plugin, PluginBuilder, UserData, Wasm};
use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

use crate::config::{PluginHostConfig, PluginPermissionGrants};
use crate::plugin_manifest::PluginCatalog;

#[derive(Clone)]
struct HostContentResolver {
    storage: Arc<dyn BlobStorage>,
    keys: Arc<Mutex<HashMap<String, StorageKey>>>,
    runtime: tokio::runtime::Handle,
    max_content_bytes: u64,
}

extism::host_fn!(asset_hub_content_read(user_data: HostContentResolver; url: String) -> String {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?;
    let key = content.keys
        .lock()
        .map_err(|_| extism::Error::msg("content reference map lock poisoned"))?
        .get(&url)
        .cloned()
        .ok_or_else(|| extism::Error::msg(format!("content reference `{url}` is not available")))?;
    let bytes = content.runtime.block_on(content.storage.get(&key))
        .map_err(|error| extism::Error::msg(error.to_string()))?
        .ok_or_else(|| extism::Error::msg(format!("content reference `{url}` was not found")))?;
    if bytes.len() as u64 > content.max_content_bytes {
        return Err(extism::Error::msg(format!(
            "content reference exceeds the {} byte plugin limit",
            content.max_content_bytes
        )));
    }
    Ok(STANDARD.encode(bytes))
});

/// Extism 资源动作执行器。
#[derive(Clone)]
pub struct ExtismResourceActionExecutor {
    bindings: Arc<Vec<ActionBinding>>,
    call_slots: Arc<Semaphore>,
    max_content_bytes: u64,
    max_input_bytes: usize,
    max_output_bytes: usize,
}

impl std::fmt::Debug for ExtismResourceActionExecutor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExtismResourceActionExecutor")
            .field("bindings", &self.bindings)
            .finish_non_exhaustive()
    }
}

impl ExtismResourceActionExecutor {
    /// 从插件 manifest 目录创建 Extism 执行器。
    pub(crate) fn from_catalog(
        catalog: &PluginCatalog,
        kind_registry: &dyn ResourceKindRegistry,
        blob_storage: Arc<dyn BlobStorage>,
        config: &PluginHostConfig,
    ) -> Result<Self, CoreError> {
        let mut bindings = Vec::new();

        for loaded_manifest in catalog
            .plugins()
            .iter()
            .filter(|plugin| plugin.manifest_path.is_some())
        {
            let manifest = &loaded_manifest.manifest;
            let PluginRuntime::Extism { wasi, .. } = &manifest.runtime else {
                continue;
            };
            let wasm = loaded_manifest.wasm.as_ref().ok_or_else(|| {
                CoreError::configuration(format!(
                    "plugin `{}` has no verified Wasm artifact",
                    manifest.plugin_id()
                ))
            })?;
            validate_external_permissions(
                manifest.plugin_id(),
                &manifest.permissions,
                &config.grants,
            )?;
            let host_content = HostContentResolver {
                storage: blob_storage.clone(),
                keys: Arc::new(Mutex::new(HashMap::new())),
                runtime: tokio::runtime::Handle::current(),
                max_content_bytes: config.max_content_bytes,
            };
            let compiled = compile_plugin(
                manifest.plugin_id(),
                wasm,
                *wasi,
                &manifest.permissions,
                &host_content,
                config,
            )?;
            preflight_handlers(
                manifest.plugin_id(),
                &compiled,
                &manifest.capabilities.resource_actions,
            )?;

            for action in &manifest.capabilities.resource_actions {
                let Some(handler) = action.plugin_handler() else {
                    continue;
                };
                bindings.push(bind_action(
                    manifest.plugin_id(),
                    &manifest.permissions,
                    action,
                    handler,
                    compiled.clone(),
                    host_content.clone(),
                    kind_registry,
                )?);
            }
        }

        Ok(Self {
            bindings: Arc::new(bindings),
            call_slots: Arc::new(Semaphore::new(config.max_concurrent_calls)),
            max_content_bytes: config.max_content_bytes,
            max_input_bytes: config.max_input_bytes,
            max_output_bytes: config.max_output_bytes,
        })
    }
}

fn validate_external_permissions(
    plugin_id: &str,
    permissions: &PluginPermissions,
    grants: &PluginPermissionGrants,
) -> Result<(), CoreError> {
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

fn validated_permission_path<'a>(
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

fn compile_plugin(
    plugin_id: &str,
    wasm: &[u8],
    wasi: bool,
    permissions: &PluginPermissions,
    host_content: &HostContentResolver,
    config: &PluginHostConfig,
) -> Result<Arc<CompiledPlugin>, CoreError> {
    let content_read = Function::new(
        "asset_hub_content_read",
        [PTR],
        [PTR],
        UserData::new(host_content.clone()),
        asset_hub_content_read,
    );
    PluginBuilder::new(manifest_for_plugin(wasm, permissions, config))
        .with_wasi(wasi)
        .with_functions([content_read])
        .compile()
        .map(Arc::new)
        .map_err(|error| {
            CoreError::configuration(format!(
                "compile plugin `{plugin_id}` verified Wasm: {error}"
            ))
        })
}

fn preflight_handlers(
    plugin_id: &str,
    compiled: &CompiledPlugin,
    actions: &[ResourceActionCapability],
) -> Result<(), CoreError> {
    let plugin = Plugin::new_from_compiled(compiled).map_err(|error| {
        CoreError::configuration(format!("instantiate plugin `{plugin_id}`: {error}"))
    })?;
    for action in actions {
        if let Some(handler) = action.plugin_handler()
            && !plugin.function_exists(handler)
        {
            return Err(CoreError::configuration(format!(
                "plugin `{plugin_id}` action `{}` references missing Wasm export `{handler}`",
                action.id
            )));
        }
    }
    Ok(())
}

fn bind_action(
    plugin_id: &str,
    permissions: &PluginPermissions,
    action: &ResourceActionCapability,
    handler: &str,
    compiled: Arc<CompiledPlugin>,
    host_content: HostContentResolver,
    kind_registry: &dyn ResourceKindRegistry,
) -> Result<ActionBinding, CoreError> {
    if permissions.filesystem.enabled() && !permissions.filesystem.has_scope() {
        return Err(CoreError::configuration(format!(
            "plugin `{plugin_id}` action `{}` declares unscoped filesystem permission",
            action.id
        )));
    }
    if permissions.network.enabled() && !permissions.network.has_scope() {
        return Err(CoreError::configuration(format!(
            "plugin `{plugin_id}` action `{}` declares unscoped network permission",
            action.id
        )));
    }
    for kind in &action.applies_to.kinds {
        asset_core::domain::ResourceKind::try_new(kind)?;
    }

    let applicable_kinds = if action.applies_to.kinds.is_empty() {
        Vec::new()
    } else {
        kind_registry
            .definitions()
            .iter()
            .filter(|definition| {
                action.applies_to.kinds.iter().any(|ancestor| {
                    kind_registry.is_a(
                        definition.kind(),
                        &asset_core::domain::ResourceKind::new(ancestor),
                    )
                })
            })
            .map(|definition| definition.kind().as_str().to_owned())
            .collect()
    };
    let applies_to = action
        .applies_to
        .to_definition()
        .with_kinds(applicable_kinds);

    Ok(ActionBinding {
        plugin_id: plugin_id.to_string(),
        action: action.id.clone(),
        handler: handler.to_string(),
        applies_to,
        permissions: permissions.clone(),
        compiled,
        host_content,
    })
}

#[async_trait]
impl ResourceActionExecutor for ExtismResourceActionExecutor {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError> {
        let Some(binding) = self.bindings.iter().find(|binding| {
            let content = request.resource().content();
            binding.action == request.action().as_str()
                && request.handler() == Some(binding.handler.as_str())
                && binding.applies_to.matches_resource(
                    request.resource().kind().as_str(),
                    content.and_then(|content| content.mime_type()),
                    content.map(|content| content.key().as_str()),
                )
        }) else {
            return Err(CoreError::configuration(format!(
                "no Extism binding for resource kind `{}` action `{}`",
                request.resource().kind(),
                request.action()
            )));
        };

        verify_permissions(binding, &request)?;
        verify_content_budget(binding, &request, self.max_content_bytes)?;

        let queued_at = std::time::Instant::now();
        let permit = self
            .call_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| CoreError::configuration("plugin executor is shutting down"))?;
        let queue_ms = queued_at.elapsed().as_millis() as u64;

        let binding = binding.clone();
        let plugin_id = binding.plugin_id.clone();
        let action_id = binding.action.clone();
        let content_lease = binding
            .host_content
            .register(&binding.plugin_id, &request)?;
        let payload = build_payload(&request, content_lease.as_ref().map(ContentLease::url));
        let max_input_bytes = self.max_input_bytes;
        let max_output_bytes = self.max_output_bytes;
        let started_at = std::time::Instant::now();
        let output = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _content_lease = content_lease;
            call_extism(binding, payload, max_input_bytes, max_output_bytes)
        })
        .await
        .map_err(|error| {
            CoreError::plugin(&plugin_id, &action_id, format!("join plugin task: {error}"))
        })
        .and_then(|output| output);
        let elapsed_ms = started_at.elapsed().as_millis() as u64;
        match &output {
            Ok(_) => tracing::info!(
                plugin = %plugin_id,
                action = %action_id,
                queue_ms,
                elapsed_ms,
                "plugin action completed"
            ),
            Err(error) => tracing::warn!(
                plugin = %plugin_id,
                action = %action_id,
                queue_ms,
                elapsed_ms,
                error = %error,
                "plugin action failed"
            ),
        }
        let output = output?;

        Ok(ResourceActionOutput::new(
            request.resource().id(),
            request.action().clone(),
            output,
        ))
    }
}

#[derive(Clone)]
struct ActionBinding {
    plugin_id: String,
    action: String,
    handler: String,
    applies_to: asset_plugin_api::ResourceActionAppliesTo,
    permissions: PluginPermissions,
    compiled: Arc<CompiledPlugin>,
    host_content: HostContentResolver,
}

impl std::fmt::Debug for ActionBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActionBinding")
            .field("plugin_id", &self.plugin_id)
            .field("action", &self.action)
            .field("handler", &self.handler)
            .finish_non_exhaustive()
    }
}

fn verify_permissions(
    binding: &ActionBinding,
    request: &ResourceActionRequest,
) -> Result<(), CoreError> {
    if !binding.permissions.resource.read {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks resource.read permission",
            binding.plugin_id, binding.action
        )));
    }
    if matches!(
        request.access(),
        asset_plugin_api::ResourceActionAccess::ReadWrite
    ) && !binding.permissions.resource.write
    {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks resource.write permission",
            binding.plugin_id, binding.action
        )));
    }
    if !matches!(
        request.content_delivery(),
        asset_plugin_api::ResourceActionContentDelivery::Auto
    ) && !binding.permissions.content.read
    {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks content.read permission",
            binding.plugin_id, binding.action
        )));
    }
    if matches!(
        request.access(),
        asset_plugin_api::ResourceActionAccess::ReadWrite
    ) && !binding.permissions.content.write
    {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks content.write permission",
            binding.plugin_id, binding.action
        )));
    }

    Ok(())
}

fn verify_content_budget(
    binding: &ActionBinding,
    request: &ResourceActionRequest,
    max_content_bytes: u64,
) -> Result<(), CoreError> {
    if matches!(
        request.content_delivery(),
        asset_plugin_api::ResourceActionContentDelivery::Auto
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
    if size > max_content_bytes {
        return Err(CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            format!("resource content is {size} bytes, plugin limit is {max_content_bytes}"),
        ));
    }
    Ok(())
}

fn call_extism(
    binding: ActionBinding,
    payload: PluginActionRequest,
    max_input_bytes: usize,
    max_output_bytes: usize,
) -> Result<PluginActionOutput, CoreError> {
    let mut plugin = Plugin::new_from_compiled(&binding.compiled).map_err(|error| {
        CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            format!("instantiate precompiled plugin: {error}"),
        )
    })?;
    let input = serde_json::to_string(&payload).map_err(|error| {
        CoreError::plugin(&binding.plugin_id, &binding.action, error.to_string())
    })?;
    if input.len() > max_input_bytes {
        return Err(CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            format!(
                "serialized input is {} bytes, limit is {max_input_bytes}",
                input.len()
            ),
        ));
    }
    let output = plugin
        .call::<&str, String>(&binding.handler, input.as_str())
        .map_err(|error| {
            CoreError::plugin(&binding.plugin_id, &binding.action, error.to_string())
        })?;
    if output.len() > max_output_bytes {
        return Err(CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            format!(
                "plugin output is {} bytes, limit is {max_output_bytes}",
                output.len()
            ),
        ));
    }

    let mut output: PluginActionOutput = serde_json::from_str(&output).map_err(|error| {
        CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            format!("plugin returned invalid action output: {error}"),
        )
    })?;
    resolve_plugin_output_urls(&mut output, &binding.plugin_id)?;

    Ok(output)
}

fn manifest_for_plugin(
    wasm: &[u8],
    permissions: &PluginPermissions,
    config: &PluginHostConfig,
) -> Manifest {
    let mut manifest = Manifest::new([Wasm::data(wasm.to_vec())])
        .with_memory_max(config.memory_max_pages)
        .with_timeout(std::time::Duration::from_secs(config.timeout_seconds));

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

impl HostContentResolver {
    fn register(
        &self,
        plugin_id: &str,
        request: &ResourceActionRequest,
    ) -> Result<Option<ContentLease>, CoreError> {
        if !matches!(
            request.content_delivery(),
            asset_plugin_api::ResourceActionContentDelivery::Url
        ) {
            return Ok(None);
        }
        let Some(content) = request.resource().content() else {
            return Ok(None);
        };
        if content.size() > self.max_content_bytes {
            return Err(CoreError::plugin(
                plugin_id,
                request.action().as_str(),
                format!(
                    "resource content is {} bytes, plugin limit is {}",
                    content.size(),
                    self.max_content_bytes
                ),
            ));
        }
        let url = content_ref_url();
        self.keys
            .lock()
            .map_err(|_| CoreError::configuration("content reference map lock poisoned"))?
            .insert(url.clone(), content.key().clone());
        Ok(Some(ContentLease {
            keys: self.keys.clone(),
            url,
        }))
    }
}

struct ContentLease {
    keys: Arc<Mutex<HashMap<String, StorageKey>>>,
    url: String,
}

impl ContentLease {
    fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for ContentLease {
    fn drop(&mut self) {
        if let Ok(mut keys) = self.keys.lock() {
            keys.remove(&self.url);
        }
    }
}

fn build_payload(
    request: &ResourceActionRequest,
    content_url: Option<&str>,
) -> PluginActionRequest {
    let resource = request.resource();
    let content_ref = resource.content();
    let content = if matches!(
        request.content_delivery(),
        asset_plugin_api::ResourceActionContentDelivery::Url
    ) {
        None
    } else {
        request.content().map(|content| PluginContentBytes {
            encoding: PluginContentEncoding::Base64,
            data: STANDARD.encode(content),
        })
    };
    let content_ref_payload = if matches!(
        request.content_delivery(),
        asset_plugin_api::ResourceActionContentDelivery::Url
    ) {
        content_ref.map(|_| PluginContentReference {
            encoding: PluginContentEncoding::Url,
            url: content_url
                .expect("URL content delivery must hold a content lease")
                .to_string(),
        })
    } else {
        None
    };

    PluginActionRequest {
        action: request.action().as_str().to_string(),
        access: request.access(),
        input: request.input().clone(),
        resource: PluginResource {
            id: resource.id().to_string(),
            name: resource.name().to_string(),
            kind: resource.kind().as_str().to_string(),
            status: status_text(resource.status()).to_string(),
            metadata: PluginResourceMetadata {
                schema_version: resource.metadata().schema_version(),
                summary: PluginResourceSummaryMetadata {
                    description: resource.metadata().description().map(str::to_string),
                    tags: resource.metadata().tags().to_vec(),
                },
            },
            content: content_ref.map(|content| PluginResourceContent {
                key: content.key().as_str().to_string(),
                size: content.size(),
                mime_type: content.mime_type().map(str::to_string),
                original_filename: content.original_filename().map(str::to_string),
                checksum: content
                    .checksums()
                    .map(|checksum| PluginChecksum {
                        kind: checksum_kind_text(checksum.kind()).to_string(),
                        value: checksum.value().to_string(),
                    })
                    .collect(),
            }),
            created_at: resource.created_at().to_rfc3339(),
            updated_at: resource.updated_at().to_rfc3339(),
            deleted_at: resource.deleted_at().map(|value| value.to_rfc3339()),
        },
        content,
        content_ref: content_ref_payload,
    }
}

fn content_ref_url() -> String {
    format!("asset://content/{}", uuid::Uuid::now_v7())
}

fn resolve_plugin_output_urls(
    output: &mut PluginActionOutput,
    plugin_id: &str,
) -> Result<(), CoreError> {
    if let PluginView::PluginFrame(frame) = &mut output.view {
        frame.url = plugin_web_asset_url(plugin_id, &frame.url)?;
    }
    Ok(())
}

fn plugin_web_asset_url(plugin_id: &str, url: &str) -> Result<String, CoreError> {
    let url = url.trim();
    if url.starts_with('/') || url.contains("://") || url.starts_with("//") {
        return Err(CoreError::plugin(
            plugin_id,
            "plugin_frame",
            "plugin_frame URL must be relative to the plugin Web root",
        ));
    }

    let relative = url.trim_start_matches("./");
    let relative = if relative.is_empty() {
        "index.html".to_string()
    } else if relative.starts_with('#') || relative.starts_with('?') {
        format!("index.html{relative}")
    } else {
        relative.to_string()
    };

    if relative.split('/').any(|part| part == "..") {
        return Err(CoreError::plugin(
            plugin_id,
            "plugin_frame",
            "plugin_frame URL contains a parent path segment",
        ));
    }
    Ok(format!("/plugins/{plugin_id}/{relative}"))
}

fn status_text(status: ResourceStatus) -> &'static str {
    match status {
        ResourceStatus::Active => "active",
        ResourceStatus::Archived => "archived",
    }
}

fn checksum_kind_text(kind: ChecksumKind) -> &'static str {
    match kind {
        ChecksumKind::Sha256 => "sha256",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_plugin_api::{ActionExecutor, PluginFrameView};

    #[test]
    fn plugin_frame_relative_url_is_resolved_to_plugin_web_route() {
        let mut output = PluginActionOutput::new(PluginView::PluginFrame(PluginFrameView {
            title: Some("demo.md".to_string()),
            url: "index.html#payload=abc".to_string(),
        }));

        resolve_plugin_output_urls(&mut output, "azvs.markdown").unwrap();

        let PluginView::PluginFrame(frame) = output.view else {
            panic!("expected plugin frame");
        };
        assert_eq!(frame.url, "/plugins/azvs.markdown/index.html#payload=abc");
    }

    #[test]
    fn plugin_frame_public_or_protocol_url_is_rejected() {
        assert!(plugin_web_asset_url("azvs.markdown", "/plugins/custom/index.html").is_err());
        assert!(plugin_web_asset_url("azvs.markdown", "asset://content/demo.md").is_err());
        assert!(plugin_web_asset_url("azvs.markdown", "https://example.com/view").is_err());
    }

    #[test]
    fn plugin_frame_hash_only_url_defaults_to_index_html() {
        assert_eq!(
            plugin_web_asset_url("azvs.markdown", "#payload=abc").unwrap(),
            "/plugins/azvs.markdown/index.html#payload=abc"
        );
    }

    #[test]
    fn external_permissions_require_matching_host_grants() {
        let permissions: PluginPermissions = serde_json::from_value(serde_json::json!({
            "resource": {"read": true, "write": false},
            "content": {"read": false, "write": false},
            "network": {"hosts": ["api.example.com"]},
            "filesystem": {"read": ["/srv/plugins/input"], "write": []}
        }))
        .unwrap();

        let error = validate_external_permissions(
            "example.plugin",
            &permissions,
            &PluginPermissionGrants::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("without a matching host grant"));

        let grants = PluginPermissionGrants {
            network_hosts: vec!["api.example.com".to_string()],
            filesystem_read: vec!["/srv/plugins".into()],
            filesystem_write: Vec::new(),
        };
        validate_external_permissions("example.plugin", &permissions, &grants).unwrap();
    }

    #[test]
    fn preflight_rejects_missing_wasm_handler_exports() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let wasm = std::fs::read(root.join("../plugins/azvs-mp4/azvs-mp4.wasm")).unwrap();
        let compiled = PluginBuilder::new(wasm).compile().unwrap();
        let manifest: asset_plugin_api::PluginManifest = serde_json::from_str(
            &std::fs::read_to_string(root.join("../plugins/azvs-mp4/azvs-mp4.json")).unwrap(),
        )
        .unwrap();
        let mut actions = manifest.capabilities.resource_actions;

        preflight_handlers("azvs.mp4", &compiled, &actions).unwrap();
        actions[0].executor = Some(ActionExecutor::Plugin {
            handler: "missing_export".to_string(),
        });
        let error = preflight_handlers("azvs.mp4", &compiled, &actions).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing Wasm export `missing_export`")
        );
    }
}
