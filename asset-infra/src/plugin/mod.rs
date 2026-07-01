use asset_core::CoreError;
use asset_core::domain::{ChecksumKind, ResourceStatus};
use asset_core::port::{
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest, ResourceKindRegistry,
};
use asset_plugin_api::{
    PluginActionOutput, PluginActionRequest, PluginChecksum, PluginContentBytes,
    PluginContentEncoding, PluginManifest, PluginResource, PluginResourceContent, PluginRuntime,
    ResourceActionCapability,
};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use extism::{Manifest, PluginBuilder, Wasm};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::KindRegistryConfig;
use crate::plugin_manifest::load_plugin_manifest_file;

/// Extism 资源动作执行器。
#[derive(Debug, Clone)]
pub struct ExtismResourceActionExecutor {
    bindings: Arc<HashMap<ActionBindingKey, ActionBinding>>,
}

impl ExtismResourceActionExecutor {
    /// 从插件 manifest 目录创建 Extism 执行器。
    pub fn from_config(
        config: &KindRegistryConfig,
        kind_registry: &dyn ResourceKindRegistry,
    ) -> Result<Self, CoreError> {
        let mut bindings = HashMap::new();

        for manifest_path in &config.plugin_manifests {
            let loaded_manifest = load_plugin_manifest(manifest_path.clone())?;
            let manifest = &loaded_manifest.manifest;
            let PluginRuntime::Extism { wasm, wasi, .. } = &manifest.runtime else {
                continue;
            };
            let wasm_path = resolve_manifest_path(&loaded_manifest.path, wasm);

            for action in &manifest.capabilities.resource_actions {
                let Some(handler) = action.plugin_handler() else {
                    continue;
                };
                bind_action(
                    &mut bindings,
                    kind_registry,
                    manifest.plugin_id(),
                    action,
                    handler,
                    &wasm_path,
                    *wasi,
                )?;
            }
        }

        Ok(Self {
            bindings: Arc::new(bindings),
        })
    }
}

fn bind_action(
    bindings: &mut HashMap<ActionBindingKey, ActionBinding>,
    kind_registry: &dyn ResourceKindRegistry,
    plugin_id: &str,
    action: &ResourceActionCapability,
    handler: &str,
    wasm_path: &Path,
    wasi: bool,
) -> Result<(), CoreError> {
    for kind in &action.applies_to.kinds {
        let kind = asset_core::domain::ResourceKind::try_new(kind)?;
        let Some(definition) = kind_registry.get(&kind) else {
            continue;
        };
        if !definition
            .actions()
            .iter()
            .any(|definition_action| definition_action.id().as_str() == action.id.as_str())
        {
            continue;
        }

        bindings.insert(
            ActionBindingKey::new(definition.kind().as_str(), action.id.as_str()),
            ActionBinding {
                plugin_id: plugin_id.to_string(),
                action: action.id.clone(),
                handler: handler.to_string(),
                wasm_path: wasm_path.to_path_buf(),
                wasi,
            },
        );
    }

    Ok(())
}

#[async_trait]
impl ResourceActionExecutor for ExtismResourceActionExecutor {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError> {
        let key = ActionBindingKey::new(
            request.resource().kind().as_str(),
            request.action().as_str(),
        );
        let Some(binding) = self.bindings.get(&key) else {
            return Err(CoreError::configuration(format!(
                "no Extism binding for resource kind `{}` action `{}`",
                request.resource().kind(),
                request.action()
            )));
        };

        if binding.handler != request.handler() {
            return Err(CoreError::configuration(format!(
                "Extism binding handler mismatch for action `{}`",
                request.action()
            )));
        }

        let binding = binding.clone();
        let payload = build_payload(&request);
        let output = tokio::task::spawn_blocking(move || call_extism(binding, payload))
            .await
            .map_err(|error| {
                CoreError::plugin(
                    "extism",
                    request.action().as_str(),
                    format!("join plugin task: {error}"),
                )
            })??;

        Ok(ResourceActionOutput::new(
            request.resource().id(),
            request.action().clone(),
            output,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ActionBindingKey {
    kind: String,
    action: String,
}

impl ActionBindingKey {
    fn new(kind: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            action: action.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct ActionBinding {
    plugin_id: String,
    action: String,
    handler: String,
    wasm_path: PathBuf,
    wasi: bool,
}

fn call_extism(
    binding: ActionBinding,
    payload: PluginActionRequest,
) -> Result<PluginActionOutput, CoreError> {
    let manifest = Manifest::new([Wasm::file(&binding.wasm_path)]);
    let mut plugin = PluginBuilder::new(manifest)
        .with_wasi(binding.wasi)
        .build()
        .map_err(|error| {
            CoreError::plugin(
                &binding.plugin_id,
                &binding.action,
                format!(
                    "build Extism plugin `{}`: {error}",
                    binding.wasm_path.display()
                ),
            )
        })?;
    let input = serde_json::to_string(&payload).map_err(|error| {
        CoreError::plugin(&binding.plugin_id, &binding.action, error.to_string())
    })?;
    let output = plugin
        .call::<&str, String>(&binding.handler, input.as_str())
        .map_err(|error| {
            CoreError::plugin(&binding.plugin_id, &binding.action, error.to_string())
        })?;

    serde_json::from_str(&output).map_err(|error| {
        CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            format!("plugin returned invalid action output: {error}"),
        )
    })
}

fn build_payload(request: &ResourceActionRequest) -> PluginActionRequest {
    let resource = request.resource();
    let content_ref = resource.content();
    let content = request.content().map(|content| PluginContentBytes {
        encoding: PluginContentEncoding::Base64,
        data: STANDARD.encode(content),
    });

    PluginActionRequest {
        action: request.action().as_str().to_string(),
        access: request.access(),
        input: request.input().clone(),
        resource: PluginResource {
            id: resource.id().to_string(),
            name: resource.name().to_string(),
            kind: resource.kind().as_str().to_string(),
            status: status_text(resource.status()).to_string(),
            metadata: resource.metadata().to_value(),
            content: content_ref.map(|content| PluginResourceContent {
                key: content.key().as_str().to_string(),
                size: content.size(),
                mime_type: content.mime_type().map(str::to_string),
                original_filename: content.original_filename().map(str::to_string),
                checksum: content
                    .checksums()
                    .iter()
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
    }
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

#[derive(Debug)]
struct LoadedPluginManifest {
    manifest: PluginManifest,
    path: PathBuf,
}

fn load_plugin_manifest(path: PathBuf) -> Result<LoadedPluginManifest, CoreError> {
    Ok(LoadedPluginManifest {
        manifest: load_plugin_manifest_file(&path)?,
        path,
    })
}

fn resolve_manifest_path(manifest_path: &Path, configured_path: &Path) -> PathBuf {
    if configured_path.is_absolute() {
        return configured_path.to_path_buf();
    }

    manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(configured_path)
}
