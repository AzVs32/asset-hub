use asset_core::CoreError;
use asset_infra::plugin_package::{generate_plugin_manifest_lock, load_verified_plugin_package};
use asset_plugin_api::manifest::PluginManifest;
use std::path::Path;

pub fn verify_manifest(path: &Path) -> Result<PluginManifest, CoreError> {
    load_verified_plugin_package(path).map(|package| package.manifest().clone())
}

pub fn generate_lock(path: &Path) -> Result<PluginManifest, CoreError> {
    generate_plugin_manifest_lock(path)
}

#[cfg(test)]
mod tests {
    include!("tests.rs");
}
