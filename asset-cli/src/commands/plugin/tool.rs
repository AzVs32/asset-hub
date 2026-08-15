use asset_core::CoreError;
use asset_infra::plugin_package::{
    InstalledPluginPackage, PluginCatalog, install_plugin_package, uninstall_plugin_package,
};
use std::path::Path;

pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
}

pub fn list_packages(packages_root: &Path) -> Result<Vec<PluginSummary>, CoreError> {
    let catalog = PluginCatalog::load(packages_root)?;
    let mut plugins = catalog
        .plugins()
        .iter()
        .map(|plugin| {
            let descriptor = &plugin.manifest().plugin;
            PluginSummary {
                id: descriptor.id.clone(),
                name: descriptor.name.clone(),
                version: descriptor.version.clone(),
                publisher: descriptor.publisher.clone(),
            }
        })
        .collect::<Vec<_>>();
    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(plugins)
}

pub fn install_package(
    source: &Path,
    packages_root: &Path,
) -> Result<InstalledPluginPackage, CoreError> {
    install_plugin_package(source, packages_root)
}

pub fn uninstall_package(
    packages_root: &Path,
    plugin_id: &str,
) -> Result<std::path::PathBuf, CoreError> {
    uninstall_plugin_package(packages_root, plugin_id)
}
