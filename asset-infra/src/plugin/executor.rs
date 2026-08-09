use asset_core::CoreError;
use asset_core::domain::{
    ActionAccess, ResourceActionAppliesTo, ResourceContentMatcher, ResourceKindDefinition,
};
use asset_core::port::{
    BlobStorage, DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRequest,
    DirectoryKindRegistry, DirectoryQuery, ResourceActionExecutor, ResourceActionOutput,
    ResourceActionRequest, ResourceKindRegistry, ResourceQuery,
};
use asset_plugin_api::manifest::{
    DirectoryActionCapability, PluginPermission, PluginPermissions, PluginRuntime,
    ResourceActionCapability,
};
use asset_plugin_api::protocol::directory::{
    DirectoryActionEffect, PluginDirectory, PluginDirectoryActionOutput,
    PluginDirectoryActionRequest,
};
use asset_plugin_api::protocol::{
    PluginActionAccess, PluginActionFailure, PluginResourceActionOutput,
    PluginResourceActionRequest,
};
use async_trait::async_trait;
use extism::{CompiledPlugin, Plugin};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

use super::content_abi::{
    ContentLease, HostContentResolver, HostContentState, build_payload, compile_plugin,
};
use super::directory_abi::HostDirectoryResolver;
use super::frame_url::resolve_plugin_output_urls;
use super::permissions::{
    host_diagnostic, validate_external_permissions, verify_content_budget, verify_permissions,
};
use super::policy::PluginExecutionPolicy;
use crate::config::PluginPermissionGrants;
use crate::kind::normalization::resource_action_applies_to;
use crate::plugin_manifest::PluginCatalog;

/// Extism 资源动作执行器。
#[derive(Clone)]
pub struct ExtismActionExecutor {
    pub(super) bindings: Arc<Vec<ActionBinding>>,
    pub(super) call_slots: Arc<Semaphore>,
    pub(super) policy: Arc<PluginExecutionPolicy>,
    pub(super) directory_bindings: Arc<Vec<DirectoryActionBinding>>,
}

/// Host adapters and policy used while compiling catalog action bindings.
#[derive(Clone)]
pub struct ExtismHost {
    directory_query: Arc<dyn DirectoryQuery>,
    resource_query: Arc<dyn ResourceQuery>,
    blob_storage: Arc<dyn BlobStorage>,
    policy: Arc<PluginExecutionPolicy>,
    grants: PluginPermissionGrants,
}

impl ExtismHost {
    pub fn new(
        directory_query: Arc<dyn DirectoryQuery>,
        resource_query: Arc<dyn ResourceQuery>,
        blob_storage: Arc<dyn BlobStorage>,
        policy: Arc<PluginExecutionPolicy>,
        grants: PluginPermissionGrants,
    ) -> Self {
        Self {
            directory_query,
            resource_query,
            blob_storage,
            policy,
            grants,
        }
    }
}

impl std::fmt::Debug for ExtismActionExecutor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExtismActionExecutor")
            .field("bindings", &self.bindings)
            .finish_non_exhaustive()
    }
}

impl ExtismActionExecutor {
    /// 从插件 manifest 目录创建 Extism 执行器。
    pub fn from_catalog(
        catalog: &PluginCatalog,
        kind_registry: &dyn ResourceKindRegistry,
        directory_kind_registry: &dyn DirectoryKindRegistry,
        host: ExtismHost,
    ) -> Result<Self, CoreError> {
        let ExtismHost {
            directory_query,
            resource_query,
            blob_storage,
            policy,
            grants,
        } = host;
        let mut bindings = Vec::new();
        let mut directory_bindings = Vec::new();

        for loaded_manifest in catalog.plugins() {
            let manifest = &loaded_manifest.manifest;
            let PluginRuntime::Extism { wasi, .. } = &manifest.runtime;
            let wasm = &loaded_manifest.wasm;
            validate_external_permissions(manifest.plugin_id(), &manifest.permissions, &grants)?;
            let host_content = HostContentResolver {
                storage: blob_storage.clone(),
                state: Arc::new(Mutex::new(HostContentState::default())),
                runtime: tokio::runtime::Handle::current(),
                policy: policy.clone(),
            };
            let host_directories = HostDirectoryResolver::new(
                directory_query.clone(),
                resource_query.clone(),
                manifest.permissions.clone(),
            );
            let compiled = compile_plugin(
                manifest.plugin_id(),
                wasm,
                *wasi,
                &manifest.permissions,
                &host_content,
                &host_directories,
                &policy,
            )?;
            preflight_handlers(
                manifest.plugin_id(),
                &compiled,
                &manifest.capabilities.resource_actions,
            )?;
            preflight_directory_handlers(
                manifest.plugin_id(),
                &compiled,
                &manifest.capabilities.directory_actions,
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
            for action in &manifest.capabilities.directory_actions {
                if !manifest.permissions.directory_read() {
                    return Err(CoreError::configuration(format!(
                        "plugin `{}` directory action `{}` requires directory.read permission",
                        manifest.plugin_id(),
                        action.id
                    )));
                }
                if action
                    .requires
                    .as_ref()
                    .is_some_and(|requires| requires.children)
                    && !manifest.permissions.directory_children_list()
                {
                    return Err(CoreError::configuration(format!(
                        "plugin `{}` directory action `{}` requires directory.children.list permission",
                        manifest.plugin_id(),
                        action.id
                    )));
                }
                if action
                    .requires
                    .as_ref()
                    .is_some_and(|requires| requires.resources)
                    && !manifest.permissions.directory_resources_list()
                {
                    return Err(CoreError::configuration(format!(
                        "plugin `{}` directory action `{}` requires directory.resources.list permission",
                        manifest.plugin_id(),
                        action.id
                    )));
                }
                let declared_kinds = action
                    .applies_to
                    .kinds
                    .iter()
                    .map(asset_core::domain::DirectoryKind::try_new)
                    .collect::<Result<Vec<_>, _>>()?;
                for kind in &declared_kinds {
                    if !directory_kind_registry.supports(kind) {
                        return Err(CoreError::configuration(format!(
                            "directory action `{}` references unknown kind `{kind}`",
                            action.id
                        )));
                    }
                }
                let applicable_kinds = if declared_kinds.is_empty() {
                    Vec::new()
                } else {
                    directory_kind_registry
                        .definitions()
                        .iter()
                        .filter(|definition| {
                            declared_kinds.iter().any(|ancestor| {
                                directory_kind_registry.is_a(definition.kind(), ancestor)
                            })
                        })
                        .map(|definition| definition.kind().as_str().to_string())
                        .collect()
                };
                directory_bindings.push(DirectoryActionBinding {
                    plugin_id: manifest.plugin_id().to_string(),
                    action: action.id.clone(),
                    handler: action.handler.clone(),
                    kinds: applicable_kinds,
                    permissions: manifest.permissions.clone(),
                    compiled: compiled.clone(),
                    host_directories: host_directories.clone(),
                });
            }
        }

        Ok(Self {
            bindings: Arc::new(bindings),
            call_slots: Arc::new(Semaphore::new(policy.max_concurrent_calls())),
            policy,
            directory_bindings: Arc::new(directory_bindings),
        })
    }
}

fn preflight_directory_handlers(
    plugin_id: &str,
    compiled: &CompiledPlugin,
    actions: &[DirectoryActionCapability],
) -> Result<(), CoreError> {
    let plugin = Plugin::new_from_compiled(compiled).map_err(|error| {
        CoreError::configuration(format!("instantiate plugin `{plugin_id}`: {error}"))
    })?;
    for action in actions {
        if !plugin.function_exists(action.handler()) {
            return Err(CoreError::configuration(format!(
                "plugin `{plugin_id}` directory action `{}` references missing Wasm export `{}`",
                action.id,
                action.handler()
            )));
        }
    }
    Ok(())
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
    let declared_kinds = action
        .applies_to
        .kinds
        .iter()
        .map(asset_core::domain::ResourceKind::try_new)
        .collect::<Result<Vec<_>, _>>()?;

    let applicable_kinds = if declared_kinds.is_empty() {
        Vec::new()
    } else {
        kind_registry
            .definitions()
            .iter()
            .filter(|definition| {
                declared_kinds
                    .iter()
                    .any(|ancestor| kind_registry.is_a(definition.kind(), ancestor))
            })
            .map(|definition| definition.kind().as_str().to_owned())
            .collect()
    };
    let mut applies_to = resource_action_applies_to(action).with_kinds(applicable_kinds);
    if applies_to.content().is_empty() && !action.applies_to.kinds.is_empty() {
        applies_to = applies_to.with_content_matcher(detect_for_action_kinds(
            kind_registry.definitions(),
            &declared_kinds,
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
    kinds: &[asset_core::domain::ResourceKind],
) -> Result<ResourceContentMatcher, CoreError> {
    let mut mime_types = Vec::new();
    let mut extensions = Vec::new();
    for kind in kinds {
        let definition = definitions
            .iter()
            .find(|definition| definition.kind() == kind)
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
impl ResourceActionExecutor for ExtismActionExecutor {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError> {
        let storage_key = request.storage_key();
        let Some(binding) = self.bindings.iter().find(|binding| {
            let content = request.resource().content();
            binding.action == request.action().as_str()
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

        let resource_id = request.resource().id();
        let operation = request
            .input()
            .get("operation")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("initial")
            .to_owned();
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
            Ok(_) => tracing::debug!(
                plugin = %plugin_id,
                action = %action_id,
                resource_id = %resource_id,
                operation = %operation,
                queue_ms,
                elapsed_ms,
                "plugin action completed"
            ),
            Err(error) => tracing::warn!(
                plugin = %plugin_id,
                action = %action_id,
                resource_id = %resource_id,
                operation = %operation,
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

#[async_trait]
impl DirectoryActionExecutor for ExtismActionExecutor {
    async fn execute(
        &self,
        request: DirectoryActionRequest,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let kind = request.directory().directory().kind();
        let Some(binding) = self.directory_bindings.iter().find(|binding| {
            binding.action == request.action().as_str()
                && (binding.kinds.is_empty()
                    || binding
                        .kinds
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(kind.as_str())))
        }) else {
            return Err(CoreError::configuration(format!(
                "no Extism binding for directory kind `{kind}` action `{}`",
                request.action()
            )));
        };
        if !binding.permissions.directory_read() {
            return Err(CoreError::configuration(format!(
                "plugin `{}` action `{}` lacks directory.read permission",
                binding.plugin_id, binding.action
            )));
        }
        if matches!(request.access(), ActionAccess::Write)
            && !binding.permissions.directory_write()
            && !binding.permissions.directory_create_child()
        {
            return Err(CoreError::configuration(format!(
                "plugin `{}` action `{}` lacks a directory write permission",
                binding.plugin_id, binding.action
            )));
        }

        let permit = self
            .call_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| CoreError::configuration("plugin executor is shutting down"))?;
        let binding = binding.clone();
        let plugin_id = binding.plugin_id.clone();
        let action_id = binding.action.clone();
        let directory_id = request.directory().id();
        let lease = binding.host_directories.register(&request)?;
        let directory = request.directory();
        let payload = PluginDirectoryActionRequest {
            action: request.action().as_str().to_string(),
            access: plugin_action_access(request.access()),
            input: request.input().clone(),
            directory: PluginDirectory {
                id: directory.id().to_string(),
                parent_id: directory.directory().parent_id().map(|id| id.to_string()),
                path: directory.path().path().to_string(),
                name: directory.directory().name().to_string(),
                kind: directory.directory().kind().as_str().to_string(),
                revision: directory.directory().revision(),
                created_at: directory.directory().created_at().to_rfc3339(),
                updated_at: directory.directory().updated_at().to_rfc3339(),
            },
            directory_ref: lease.reference().to_string(),
        };
        let policy = self.policy.clone();
        let output = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _lease = lease;
            call_extism_directory(binding, payload, &policy)
        })
        .await
        .map_err(|error| {
            CoreError::plugin(&plugin_id, &action_id, format!("join plugin task: {error}"))
        })??;

        Ok(DirectoryActionOutput::new(
            directory_id,
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
    pub(super) applies_to: ResourceActionAppliesTo,
    pub(super) permissions: PluginPermissions,
    pub(super) compiled: Arc<CompiledPlugin>,
    pub(super) host_content: HostContentResolver,
}

fn plugin_action_access(access: ActionAccess) -> PluginActionAccess {
    match access {
        ActionAccess::Read => PluginActionAccess::Read,
        ActionAccess::Write => PluginActionAccess::Write,
    }
}

#[derive(Clone)]
pub(super) struct DirectoryActionBinding {
    plugin_id: String,
    action: String,
    handler: String,
    kinds: Vec<String>,
    permissions: PluginPermissions,
    compiled: Arc<CompiledPlugin>,
    host_directories: HostDirectoryResolver,
}

fn call_extism_directory(
    binding: DirectoryActionBinding,
    payload: PluginDirectoryActionRequest,
    policy: &PluginExecutionPolicy,
) -> Result<PluginDirectoryActionOutput, CoreError> {
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
        return Err(CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            "serialized directory action input exceeds plugin limit",
        ));
    }
    let raw = plugin
        .call::<&str, String>(&binding.handler, &input)
        .map_err(|error| {
            CoreError::plugin(&binding.plugin_id, &binding.action, error.to_string())
        })?;
    if raw.len() > policy.max_output_bytes() {
        return Err(CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            "directory action output exceeds plugin limit",
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            format!("invalid JSON output: {error}"),
        )
    })?;
    if value.get("error").is_some() {
        let failure: PluginActionFailure = serde_json::from_value(value).map_err(|error| {
            CoreError::plugin(
                &binding.plugin_id,
                &binding.action,
                format!("invalid failure diagnostic: {error}"),
            )
        })?;
        return Err(CoreError::plugin_failure(
            &binding.plugin_id,
            &binding.action,
            failure,
        ));
    }
    let mut output: PluginDirectoryActionOutput =
        serde_json::from_value(value).map_err(|error| {
            CoreError::plugin(
                &binding.plugin_id,
                &binding.action,
                format!("invalid directory action output: {error}"),
            )
        })?;
    for effect in &output.effects {
        let allowed = match effect {
            DirectoryActionEffect::Update(_) => {
                binding.permissions.allows(PluginPermission::DirectoryWrite)
            }
            DirectoryActionEffect::CreateChild(_) => binding
                .permissions
                .allows(PluginPermission::DirectoryCreateChild),
        };
        if !allowed {
            return Err(CoreError::plugin(
                &binding.plugin_id,
                &binding.action,
                "directory action returned an effect without the required permission",
            ));
        }
    }
    resolve_plugin_output_urls_for_directory(&mut output, &binding.plugin_id)?;
    Ok(output)
}

fn resolve_plugin_output_urls_for_directory(
    output: &mut PluginDirectoryActionOutput,
    plugin_id: &str,
) -> Result<(), CoreError> {
    let mut shared = PluginResourceActionOutput {
        view: output.view.clone(),
        effects: Vec::new(),
        diagnostics: output.diagnostics.clone(),
    };
    resolve_plugin_output_urls(&mut shared, plugin_id)?;
    output.view = shared.view;
    Ok(())
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
    payload: PluginResourceActionRequest,
    policy: &PluginExecutionPolicy,
) -> Result<PluginResourceActionOutput, CoreError> {
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
                asset_plugin_api::protocol::diagnostic::codes::INPUT_LIMIT_EXCEEDED,
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
                asset_plugin_api::protocol::diagnostic::codes::OUTPUT_LIMIT_EXCEEDED,
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
                asset_plugin_api::protocol::diagnostic::codes::INVALID_OUTPUT,
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
                    asset_plugin_api::protocol::diagnostic::codes::INVALID_OUTPUT,
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
    let mut output: PluginResourceActionOutput =
        serde_json::from_value(value).map_err(|error| {
            CoreError::plugin_diagnostic(
                &binding.plugin_id,
                &binding.action,
                host_diagnostic(
                    asset_plugin_api::protocol::diagnostic::codes::INVALID_OUTPUT,
                    format!("plugin returned invalid action output: {error}"),
                ),
            )
        })?;
    if output.effects.iter().any(|effect| {
        matches!(
            effect,
            asset_plugin_api::protocol::PluginResourceActionEffect::ReplaceContent(_)
        )
    }) && !binding.permissions.resource_content_replace()
    {
        return Err(CoreError::plugin_diagnostic(
            &binding.plugin_id,
            &binding.action,
            host_diagnostic(
                asset_plugin_api::protocol::diagnostic::codes::PERMISSION_DENIED,
                "plugin returned replace_content without resource.content.replace permission",
            ),
        ));
    }
    resolve_plugin_output_urls(&mut output, &binding.plugin_id)?;

    Ok(output)
}
