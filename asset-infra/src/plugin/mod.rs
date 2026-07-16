use asset_core::CoreError;
use asset_core::domain::{ChecksumKind, ResourceStatus, StorageKey};
use asset_core::port::{
    BlobStorage, ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest,
    ResourceContentMatcher, ResourceKindDefinition, ResourceKindRegistry,
};
use asset_core::service::PluginExecutionPolicy;
use asset_plugin_api::{
    PluginActionFailure, PluginActionOutput, PluginActionRequest, PluginChecksum,
    PluginContentBytes, PluginContentEncoding, PluginContentRange, PluginContentReference,
    PluginDiagnostic, PluginDiagnosticSeverity, PluginPermission, PluginPermissions,
    PluginResource, PluginResourceContent, PluginResourceMetadata, PluginResourceSummaryMetadata,
    PluginRuntime, PluginView, ResourceActionCapability,
};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use extism::{CompiledPlugin, Function, Manifest, PTR, Plugin, PluginBuilder, UserData, Wasm};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

use crate::config::PluginPermissionGrants;
use crate::plugin_manifest::PluginCatalog;

#[derive(Clone)]
struct HostContentResolver {
    storage: Arc<dyn BlobStorage>,
    state: Arc<Mutex<HostContentState>>,
    runtime: tokio::runtime::Handle,
    policy: Arc<PluginExecutionPolicy>,
}

#[derive(Default)]
struct HostContentState {
    references: HashMap<String, AvailableContent>,
    handles: HashMap<String, OpenContent>,
}

#[derive(Clone)]
struct AvailableContent {
    key: StorageKey,
    size: u64,
}

#[derive(Clone)]
struct OpenContent {
    reference: String,
    key: StorageKey,
    size: u64,
}

extism::host_fn!(asset_hub_content_open(user_data: HostContentResolver; reference: String) -> String {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?
        .clone();
    content.open(&reference).map_err(|error| extism::Error::msg(error.to_string()))
});

extism::host_fn!(asset_hub_content_size(user_data: HostContentResolver; handle: String) -> u64 {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?
        .clone();
    content.size(&handle).map_err(|error| extism::Error::msg(error.to_string()))
});

extism::host_fn!(asset_hub_content_read(user_data: HostContentResolver; handle: String, offset: u64, length: u64) -> Vec<u8> {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?
        .clone();
    content
        .read(&handle, offset, length)
        .map_err(|error| extism::Error::msg(error.to_string()))
});

extism::host_fn!(asset_hub_content_close(user_data: HostContentResolver; handle: String) {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?
        .clone();
    content.close(&handle).map_err(|error| extism::Error::msg(error.to_string()))?;
    Ok(())
});

/// Extism 资源动作执行器。
#[derive(Clone)]
pub struct ExtismResourceActionExecutor {
    bindings: Arc<Vec<ActionBinding>>,
    call_slots: Arc<Semaphore>,
    policy: Arc<PluginExecutionPolicy>,
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
        policy: Arc<PluginExecutionPolicy>,
        grants: &PluginPermissionGrants,
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
            validate_external_permissions(manifest.plugin_id(), &manifest.permissions, grants)?;
            let host_content = HostContentResolver {
                storage: blob_storage.clone(),
                state: Arc::new(Mutex::new(HostContentState::default())),
                runtime: tokio::runtime::Handle::current(),
                policy: policy.clone(),
            };
            let compiled = compile_plugin(
                manifest.plugin_id(),
                wasm,
                *wasi,
                &manifest.permissions,
                &host_content,
                &policy,
            )?;
            preflight_handlers(
                manifest.plugin_id(),
                &compiled,
                &manifest.capabilities.resource_actions,
            )?;

            for action in &manifest.capabilities.resource_actions {
                bindings.push(bind_action(
                    manifest.plugin_id(),
                    &manifest.permissions,
                    action,
                    action.handler(),
                    compiled.clone(),
                    host_content.clone(),
                    kind_registry,
                )?);
            }
        }

        Ok(Self {
            bindings: Arc::new(bindings),
            call_slots: Arc::new(Semaphore::new(policy.max_concurrent_calls())),
            policy,
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
    policy: &PluginExecutionPolicy,
) -> Result<Arc<CompiledPlugin>, CoreError> {
    let content_open = Function::new(
        "asset_hub_content_open",
        [PTR],
        [PTR],
        UserData::new(host_content.clone()),
        asset_hub_content_open,
    );
    let content_size = Function::new(
        "asset_hub_content_size",
        [PTR],
        [PTR],
        UserData::new(host_content.clone()),
        asset_hub_content_size,
    );
    let content_read = Function::new(
        "asset_hub_content_read",
        [PTR, PTR, PTR],
        [PTR],
        UserData::new(host_content.clone()),
        asset_hub_content_read,
    );
    let content_close = Function::new(
        "asset_hub_content_close",
        [PTR],
        [],
        UserData::new(host_content.clone()),
        asset_hub_content_close,
    );
    PluginBuilder::new(manifest_for_plugin(wasm, permissions, policy))
        .with_wasi(wasi)
        .with_functions([content_open, content_size, content_read, content_close])
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
        let handler = action.handler();
        if !plugin.function_exists(handler) {
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
    let mut applies_to = action
        .applies_to
        .to_definition()
        .with_kinds(applicable_kinds);
    if applies_to.content().is_empty() && !action.applies_to.kinds.is_empty() {
        applies_to = applies_to.with_content_matcher(detect_for_action_kinds(
            kind_registry.definitions(),
            &action.applies_to.kinds,
        )?);
    }

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

fn detect_for_action_kinds(
    definitions: &[ResourceKindDefinition],
    kinds: &[String],
) -> Result<ResourceContentMatcher, CoreError> {
    let mut mime_types = Vec::new();
    let mut extensions = Vec::new();
    for kind in kinds {
        let definition = definitions
            .iter()
            .find(|definition| definition.kind().as_str().eq_ignore_ascii_case(kind))
            .ok_or_else(|| {
                CoreError::configuration(format!(
                    "resource action references unknown kind `{kind}`"
                ))
            })?;
        mime_types.extend(definition.detect().mime_types().iter().cloned());
        extensions.extend(definition.detect().extensions().iter().cloned());
    }
    mime_types.sort();
    mime_types.dedup();
    extensions.sort();
    extensions.dedup();
    Ok(ResourceContentMatcher::new()
        .with_mime_types(mime_types)
        .with_extensions(extensions))
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
        verify_content_budget(binding, &request, &self.policy)?;

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
        let payload = build_payload(
            &request,
            content_lease.as_ref().map(ContentLease::reference),
        );
        let policy = self.policy.clone();
        let started_at = std::time::Instant::now();
        let output = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _content_lease = content_lease;
            call_extism(binding, payload, &policy)
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
    if !binding
        .permissions
        .allows(PluginPermission::ResourceMetadataRead)
    {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks resource.metadata.read permission",
            binding.plugin_id, binding.action
        )));
    }
    if matches!(
        request.access(),
        asset_plugin_api::ResourceActionAccess::ReadWrite
    ) && !binding.permissions.resource_metadata_write()
        && !binding.permissions.content_replace()
        && !binding.permissions.derived_asset_write()
    {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks a write permission",
            binding.plugin_id, binding.action
        )));
    }
    if !matches!(
        request.content_delivery(),
        asset_plugin_api::ResourceActionContentDelivery::Auto
    ) && !binding.permissions.allows(PluginPermission::ContentRead)
    {
        return Err(CoreError::configuration(format!(
            "plugin `{}` action `{}` lacks content.read permission",
            binding.plugin_id, binding.action
        )));
    }
    Ok(())
}

fn verify_content_budget(
    binding: &ActionBinding,
    request: &ResourceActionRequest,
    policy: &PluginExecutionPolicy,
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
    if size > policy.max_content_bytes() {
        return Err(CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            host_diagnostic(
                asset_plugin_api::diagnostic::codes::CONTENT_LIMIT_EXCEEDED,
                format!(
                    "resource content is {size} bytes, plugin limit is {}",
                    policy.max_content_bytes()
                ),
            ),
        ));
    }
    Ok(())
}

fn call_extism(
    binding: ActionBinding,
    payload: PluginActionRequest,
    policy: &PluginExecutionPolicy,
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
    if input.len() > policy.max_input_bytes() {
        return Err(CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            host_diagnostic(
                asset_plugin_api::diagnostic::codes::INPUT_LIMIT_EXCEEDED,
                format!(
                    "serialized input is {} bytes, limit is {}",
                    input.len(),
                    policy.max_input_bytes()
                ),
            ),
        ));
    }
    let output = plugin
        .call::<&str, String>(&binding.handler, input.as_str())
        .map_err(|error| {
            CoreError::plugin(&binding.plugin_id, &binding.action, error.to_string())
        })?;
    if output.len() > policy.max_output_bytes() {
        return Err(CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            host_diagnostic(
                asset_plugin_api::diagnostic::codes::OUTPUT_LIMIT_EXCEEDED,
                format!(
                    "plugin output is {} bytes, limit is {}",
                    output.len(),
                    policy.max_output_bytes()
                ),
            ),
        ));
    }

    let value: serde_json::Value = serde_json::from_str(&output).map_err(|error| {
        CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            host_diagnostic(
                asset_plugin_api::diagnostic::codes::INVALID_OUTPUT,
                format!("plugin returned invalid JSON: {error}"),
            ),
        )
    })?;
    if value.get("error").is_some() {
        let failure: PluginActionFailure = serde_json::from_value(value).map_err(|error| {
            CoreError::plugin_diagnostic(
                &binding.plugin_id,
                &binding.action,
                host_diagnostic(
                    asset_plugin_api::diagnostic::codes::INVALID_OUTPUT,
                    format!("plugin returned an invalid failure diagnostic: {error}"),
                ),
            )
        })?;
        return Err(CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            failure.error,
        ));
    }
    let mut output: PluginActionOutput = serde_json::from_value(value).map_err(|error| {
        CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            host_diagnostic(
                asset_plugin_api::diagnostic::codes::INVALID_OUTPUT,
                format!("plugin returned invalid action output: {error}"),
            ),
        )
    })?;
    if output.effects.iter().any(|effect| {
        matches!(
            effect,
            asset_plugin_api::PluginActionEffect::ReplaceContent(_)
        )
    }) && !binding.permissions.content_replace()
    {
        return Err(CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            host_diagnostic(
                asset_plugin_api::diagnostic::codes::PERMISSION_DENIED,
                "plugin returned replace_content without content.replace permission",
            ),
        ));
    }
    resolve_plugin_output_urls(&mut output, &binding.plugin_id)?;

    Ok(output)
}

fn host_diagnostic(code: &str, message: impl Into<String>) -> PluginDiagnostic {
    PluginDiagnostic {
        code: code.to_string(),
        message: message.into(),
        severity: PluginDiagnosticSeverity::Error,
        retryable: false,
        details: None,
    }
}

fn manifest_for_plugin(
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

impl HostContentResolver {
    fn register(
        &self,
        plugin_id: &str,
        request: &ResourceActionRequest,
    ) -> Result<Option<ContentLease>, CoreError> {
        if !matches!(
            request.content_delivery(),
            asset_plugin_api::ResourceActionContentDelivery::Reference
        ) {
            return Ok(None);
        }
        let Some(content) = request.resource().content() else {
            return Ok(None);
        };
        if content.size() > self.policy.max_content_bytes() {
            return Err(CoreError::plugin(
                plugin_id,
                request.action().as_str(),
                format!(
                    "resource content is {} bytes, plugin limit is {}",
                    content.size(),
                    self.policy.max_content_bytes()
                ),
            ));
        }
        let reference = content_reference();
        self.state
            .lock()
            .map_err(|_| CoreError::configuration("content reference map lock poisoned"))?
            .references
            .insert(
                reference.clone(),
                AvailableContent {
                    key: content.key().clone(),
                    size: content.size(),
                },
            );
        Ok(Some(ContentLease {
            state: self.state.clone(),
            reference,
        }))
    }

    fn open(&self, reference: &str) -> Result<String, CoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| CoreError::configuration("content host state lock poisoned"))?;
        let content = state.references.get(reference).cloned().ok_or_else(|| {
            CoreError::configuration(format!("content reference `{reference}` is not available"))
        })?;
        let handle = format!("content:handle:{}", uuid::Uuid::now_v7());
        state.handles.insert(
            handle.clone(),
            OpenContent {
                reference: reference.to_string(),
                key: content.key,
                size: content.size,
            },
        );
        Ok(handle)
    }

    fn size(&self, handle: &str) -> Result<u64, CoreError> {
        self.open_content(handle).map(|content| content.size)
    }

    fn read(&self, handle: &str, offset: u64, length: u64) -> Result<Vec<u8>, CoreError> {
        let content = self.open_content(handle)?;
        let range = PluginContentRange::new(offset, length)
            .and_then(|range| range.bounded(content.size, self.policy.max_content_read_bytes()))
            .map_err(|error| CoreError::configuration(error.to_string()))?;
        if range.length == 0 {
            return Ok(Vec::new());
        }
        let offset = range.offset;
        let length = range.length;
        let end = range.end() - 1;
        self.runtime.block_on(async {
            let Some(mut stream) = self
                .storage
                .get_range_stream(&content.key, offset, end)
                .await?
            else {
                return Err(CoreError::not_found(
                    "plugin content",
                    content.key.to_string(),
                ));
            };
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                bytes.extend_from_slice(&chunk?);
                if bytes.len() as u64 > length {
                    return Err(CoreError::configuration(
                        "content storage returned more bytes than requested",
                    ));
                }
            }
            Ok(bytes)
        })
    }

    fn close(&self, handle: &str) -> Result<(), CoreError> {
        let removed = self
            .state
            .lock()
            .map_err(|_| CoreError::configuration("content host state lock poisoned"))?
            .handles
            .remove(handle);
        if removed.is_none() {
            return Err(CoreError::configuration(format!(
                "content handle `{handle}` is not open"
            )));
        }
        Ok(())
    }

    fn open_content(&self, handle: &str) -> Result<OpenContent, CoreError> {
        self.state
            .lock()
            .map_err(|_| CoreError::configuration("content host state lock poisoned"))?
            .handles
            .get(handle)
            .cloned()
            .ok_or_else(|| {
                CoreError::configuration(format!("content handle `{handle}` is not open"))
            })
    }
}

struct ContentLease {
    state: Arc<Mutex<HostContentState>>,
    reference: String,
}

impl ContentLease {
    fn reference(&self) -> &str {
        &self.reference
    }
}

impl Drop for ContentLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.references.remove(&self.reference);
            state
                .handles
                .retain(|_, handle| handle.reference != self.reference);
        }
    }
}

fn build_payload(
    request: &ResourceActionRequest,
    content_reference: Option<&str>,
) -> PluginActionRequest {
    let resource = request.resource();
    let content_ref = resource.content();
    let content = if matches!(
        request.content_delivery(),
        asset_plugin_api::ResourceActionContentDelivery::Reference
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
        asset_plugin_api::ResourceActionContentDelivery::Reference
    ) {
        content_ref.map(|_| PluginContentReference {
            abi_version: asset_plugin_api::CONTENT_ABI_VERSION,
            encoding: PluginContentEncoding::Handle,
            reference: content_reference
                .expect("reference content delivery must hold a content lease")
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

fn content_reference() -> String {
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
    use asset_plugin_api::PluginFrameView;

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

    #[tokio::test(flavor = "multi_thread")]
    async fn content_handles_read_raw_bounded_ranges_and_close() {
        let root = std::env::temp_dir().join(format!(
            "asset-hub-content-handle-test-{}",
            uuid::Uuid::now_v7()
        ));
        let storage = Arc::new(
            crate::storage::OpenDalBlobStorage::from_config(&crate::config::BlobConfig {
                fs_root: root.clone(),
            })
            .unwrap(),
        );
        let key = StorageKey::new("docs/demo.bin").unwrap();
        storage
            .put(&key, bytes::Bytes::from_static(b"abcdefgh"))
            .await
            .unwrap();
        let reference = "asset://content/test".to_string();
        let mut state = HostContentState::default();
        state
            .references
            .insert(reference.clone(), AvailableContent { key, size: 8 });
        let resolver = HostContentResolver {
            storage,
            state: Arc::new(Mutex::new(state)),
            runtime: tokio::runtime::Handle::current(),
            policy: Arc::new(PluginExecutionPolicy::new(128, 16, 3, 1024, 1024, 1, 32, 5).unwrap()),
        };

        tokio::task::spawn_blocking(move || {
            let handle = resolver.open(&reference).unwrap();
            assert_eq!(resolver.size(&handle).unwrap(), 8);
            assert_eq!(resolver.read(&handle, 2, 100).unwrap(), b"cde");
            assert_eq!(resolver.read(&handle, 5, 3).unwrap(), b"fgh");
            resolver.close(&handle).unwrap();
            assert!(resolver.size(&handle).is_err());
        })
        .await
        .unwrap();

        let _ = std::fs::remove_dir_all(root);
    }
}
