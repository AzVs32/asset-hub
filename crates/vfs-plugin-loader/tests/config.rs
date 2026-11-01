use ah_config::Registry;
use ah_vfs::application::driver_catalog::DriverCatalog;
use vfs_plugin_loader::{PluginConfig, PluginLoader, PluginSource, VfsConfig};

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn linking_the_loader_registers_its_namespaces_and_loads_an_empty_catalog() -> Result {
    let directory = tempfile::tempdir()?;
    let registry = Registry::for_space("default")?;
    assert_eq!(
        registry
            .sections()
            .iter()
            .map(|section| section.key())
            .collect::<Vec<_>>(),
        ["vfs", "vfs.plugins"]
    );
    let config = registry.load_optional(directory.path().join("missing.toml"))?;
    config.get::<VfsConfig>()?;
    assert!(config.get::<PluginConfig>()?.entries.is_empty());
    let catalog = PluginLoader::load_from(&config, DriverCatalog::new()).await?;
    assert!(catalog.list().is_empty());
    Ok(())
}

#[test]
fn plugin_entries_keep_their_configuration_path_defaults_and_unknown_field_validation() -> Result {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
[[vfs.plugins.entries]]
expected_key = "webdav"
transport = "wasm"
module = "plugins/webdav.wasm"
[vfs.plugins.entries.settings]
label = "configured"
"#,
    )?;
    let config = Registry::for_space("default")?.load_required(&path)?;
    let plugins = config.get::<PluginConfig>()?;
    let entry = plugins.entries.first().ok_or("plugin entry")?;
    assert_eq!(entry.expected_key.as_deref(), Some("webdav"));
    assert_eq!(entry.settings["label"], "configured");
    let PluginSource::Wasm {
        module, profile, ..
    } = &entry.source
    else {
        return Err("expected WASM source".into());
    };
    assert_eq!(module, std::path::Path::new("plugins/webdav.wasm"));
    assert_eq!(profile, vfs_plugin_protocol::WASM_HOST_PROFILE);
    std::fs::write(&path, "[vfs.plugins]\nunknown = true\n")?;
    assert!(Registry::for_space("default")?.load_required(path).is_err());
    Ok(())
}
