use asset_core::CoreError;
use asset_core::port::{
    BlobStorage, ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest,
    ResourceKindDefinition, ResourceKindRegistry,
};
use asset_plugin_api::{
    PluginActionFailure, PluginActionOutput, PluginActionRequest, PluginExecutionPolicy,
    PluginPermissions, PluginRuntime, ResourceActionCapability, ResourceContentMatcher,
};
use async_trait::async_trait;
use extism::{CompiledPlugin, Plugin};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

use super::content_abi::{
    ContentLease, HostContentResolver, HostContentState, build_payload, compile_plugin,
};
use super::frame_url::resolve_plugin_output_urls;
use super::permissions::{
    host_diagnostic, validate_external_permissions, verify_content_budget, verify_permissions,
};
use crate::config::PluginPermissionGrants;
use crate::plugin_manifest::PluginCatalog;

/// Extism 资源动作执行器。
#[derive(Clone)]
pub struct ExtismResourceActionExecutor {
    pub(super) bindings: Arc<Vec<ActionBinding>>,
    pub(super) call_slots: Arc<Semaphore>,
    pub(super) policy: Arc<PluginExecutionPolicy>,
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

pub(super) fn preflight_handlers(
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

pub(super) fn bind_action(
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

pub(super) fn detect_for_action_kinds(
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
        let storage_key = request.resource().storage_key();
        let Some(binding) = self.bindings.iter().find(|binding| {
            let content = request.resource().content();
            binding.action == request.action().as_str()
                && request.handler() == Some(binding.handler.as_str())
                && binding.applies_to.matches_resource(
                    request.resource().kind().as_str(),
                    content.and_then(|content| content.mime_type()),
                    content.map(|_| storage_key.as_str()),
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

pub(super) struct ActionBinding {
    pub(super) plugin_id: String,
    pub(super) action: String,
    pub(super) handler: String,
    pub(super) applies_to: asset_plugin_api::ResourceActionAppliesTo,
    pub(super) permissions: PluginPermissions,
    pub(super) compiled: Arc<CompiledPlugin>,
    pub(super) host_content: HostContentResolver,
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

pub(super) fn call_extism(
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
        return Err(CoreError::plugin_failure(
            &binding.plugin_id,
            &binding.action,
            failure,
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
