use asset_core::CoreError;
use asset_core::domain::{ChecksumKind, ResourceStatus};
use asset_core::port::{
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest, ResourceKindRegistry,
};
use asset_plugin_api::{
    PluginActionOutput, PluginActionRequest, PluginChecksum, PluginContentBytes,
    PluginContentEncoding, PluginContentReference, PluginManifest, PluginPermissions,
    PluginResource, PluginResourceContent, PluginRuntime, PluginView, ResourceActionCapability,
};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use extism::{Function, Manifest, PTR, PluginBuilder, UserData, Wasm};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::KindRegistryConfig;
use crate::plugin_manifest::load_plugin_manifest_file;

type HostContentMap = HashMap<String, String>;

extism::host_fn!(asset_hub_content_read(user_data: HostContentMap; url: String) -> String {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?;
    content
        .get(&url)
        .cloned()
        .ok_or_else(|| extism::Error::msg(format!("content reference `{url}` is not available")))
});

/// Extism 资源动作执行器。
#[derive(Debug, Clone)]
pub struct ExtismResourceActionExecutor {
    bindings: Arc<Vec<ActionBinding>>,
}

impl ExtismResourceActionExecutor {
    /// 从插件 manifest 目录创建 Extism 执行器。
    pub fn from_config(
        config: &KindRegistryConfig,
        _kind_registry: &dyn ResourceKindRegistry,
    ) -> Result<Self, CoreError> {
        let mut bindings = Vec::new();

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
                    manifest.plugin_id(),
                    &manifest.permissions,
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
    bindings: &mut Vec<ActionBinding>,
    plugin_id: &str,
    permissions: &PluginPermissions,
    action: &ResourceActionCapability,
    handler: &str,
    wasm_path: &Path,
    wasi: bool,
) -> Result<(), CoreError> {
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

    bindings.push(ActionBinding {
        plugin_id: plugin_id.to_string(),
        action: action.id.clone(),
        handler: handler.to_string(),
        applies_to: action.applies_to.to_definition(),
        permissions: permissions.clone(),
        wasm_path: wasm_path.to_path_buf(),
        wasi,
    });

    Ok(())
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

        let binding = binding.clone();
        let payload = build_payload(&request);
        let host_content = host_content_map(&request);
        let output =
            tokio::task::spawn_blocking(move || call_extism(binding, payload, host_content))
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

#[derive(Debug, Clone)]
struct ActionBinding {
    plugin_id: String,
    action: String,
    handler: String,
    applies_to: asset_plugin_api::ResourceActionAppliesTo,
    permissions: PluginPermissions,
    wasm_path: PathBuf,
    wasi: bool,
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
    if request.resource().content().is_some() && !binding.permissions.content.read {
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

fn call_extism(
    binding: ActionBinding,
    payload: PluginActionRequest,
    host_content: HostContentMap,
) -> Result<PluginActionOutput, CoreError> {
    let manifest = manifest_for_binding(&binding);
    let content_read = Function::new(
        "asset_hub_content_read",
        [PTR],
        [PTR],
        UserData::new(host_content),
        asset_hub_content_read,
    );
    let mut plugin = PluginBuilder::new(manifest)
        .with_wasi(binding.wasi)
        .with_functions([content_read])
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

    let mut output: PluginActionOutput = serde_json::from_str(&output).map_err(|error| {
        CoreError::plugin(
            &binding.plugin_id,
            &binding.action,
            format!("plugin returned invalid action output: {error}"),
        )
    })?;
    resolve_plugin_output_urls(&mut output, &binding.plugin_id);

    Ok(output)
}

fn manifest_for_binding(binding: &ActionBinding) -> Manifest {
    let mut manifest = Manifest::new([Wasm::file(&binding.wasm_path)]);

    if binding.permissions.network.enabled() {
        manifest = manifest.with_allowed_hosts(binding.permissions.network.hosts().iter().cloned());
    }

    for path in binding.permissions.filesystem.read_paths() {
        manifest = manifest.with_allowed_path(format!("ro:{path}"), path);
    }
    for path in binding.permissions.filesystem.write_paths() {
        manifest = manifest.with_allowed_path(path.clone(), path);
    }

    manifest
}

fn host_content_map(request: &ResourceActionRequest) -> HostContentMap {
    let mut map = HashMap::new();
    let Some(content_ref) = request.resource().content() else {
        return map;
    };
    let Some(content) = request.content() else {
        return map;
    };

    map.insert(
        content_ref_url(content_ref.key().as_str()),
        STANDARD.encode(content),
    );
    map
}

fn build_payload(request: &ResourceActionRequest) -> PluginActionRequest {
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
    let content_ref_payload = if content.is_none() {
        content_ref.map(|content| PluginContentReference {
            encoding: PluginContentEncoding::Url,
            url: content_ref_url(content.key().as_str()),
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
        content_ref: content_ref_payload,
    }
}

fn content_ref_url(storage_key: &str) -> String {
    format!("asset://content/{storage_key}")
}

fn resolve_plugin_output_urls(output: &mut PluginActionOutput, plugin_id: &str) {
    if let PluginView::PluginFrame(frame) = &mut output.view {
        frame.url = plugin_web_asset_url(plugin_id, &frame.url);
    }
}

fn plugin_web_asset_url(plugin_id: &str, url: &str) -> String {
    let url = url.trim();
    if is_public_or_protocol_url(url) {
        return url.to_string();
    }

    let relative = url.trim_start_matches("./");
    let relative = if relative.is_empty() {
        "index.html".to_string()
    } else if relative.starts_with('#') || relative.starts_with('?') {
        format!("index.html{relative}")
    } else {
        relative.to_string()
    };

    format!("/plugins/{plugin_id}/{relative}")
}

fn is_public_or_protocol_url(url: &str) -> bool {
    url.starts_with('/') || url.contains("://")
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

        resolve_plugin_output_urls(&mut output, "azvs.markdown");

        let PluginView::PluginFrame(frame) = output.view else {
            panic!("expected plugin frame");
        };
        assert_eq!(frame.url, "/plugins/azvs.markdown/index.html#payload=abc");
    }

    #[test]
    fn plugin_frame_public_or_protocol_url_is_kept() {
        assert_eq!(
            plugin_web_asset_url("azvs.markdown", "/plugins/custom/index.html"),
            "/plugins/custom/index.html"
        );
        assert_eq!(
            plugin_web_asset_url("azvs.markdown", "asset://content/demo.md"),
            "asset://content/demo.md"
        );
        assert_eq!(
            plugin_web_asset_url("azvs.markdown", "https://example.com/view"),
            "https://example.com/view"
        );
    }

    #[test]
    fn plugin_frame_hash_only_url_defaults_to_index_html() {
        assert_eq!(
            plugin_web_asset_url("azvs.markdown", "#payload=abc"),
            "/plugins/azvs.markdown/index.html#payload=abc"
        );
    }
}
