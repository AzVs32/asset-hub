mod dto;
mod error;
mod handlers;
mod openapi;
mod router;
mod settings;
mod state;

#[cfg(test)]
mod tests;

use asset_apps::AssetRuntime;
use settings::HttpSettings;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = HttpSettings::from_env()?;
    let runtime = AssetRuntime::from_optional_config_file(settings.config_path()).await?;
    let listener = tokio::net::TcpListener::bind(settings.addr()).await?;

    println!("asset-http listening on http://{}", settings.addr());
    println!(
        "asset-http config file: {}",
        settings
            .config_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| settings.default_config_file().to_string())
    );
    println!("asset-http config: {:?}", runtime.config());

    axum::serve(
        listener,
        router::build_with_options_and_plugin_web_roots(
            runtime.resource_service(),
            runtime.resource_kind_registry(),
            settings.router_options().clone(),
            plugin_web_roots(runtime.config())?,
        ),
    )
    .await?;

    Ok(())
}

fn plugin_web_roots(
    config: &asset_infra::config::AssetInfraConfig,
) -> Result<HashMap<String, PathBuf>, Box<dyn std::error::Error>> {
    let mut roots = HashMap::new();
    for manifest_path in &config.kind.plugin_manifests {
        let content = std::fs::read_to_string(manifest_path)?;
        let manifest: serde_json::Value = serde_json::from_str(&content)?;
        let Some(plugin_id) = manifest
            .pointer("/plugin/id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Some(web_root) = manifest
            .pointer("/web/root")
            .and_then(|value| value.as_str())
            .map(PathBuf::from)
        else {
            continue;
        };
        let root = resolve_manifest_path(manifest_path, &web_root);
        roots.insert(plugin_id, root);
    }
    Ok(roots)
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
