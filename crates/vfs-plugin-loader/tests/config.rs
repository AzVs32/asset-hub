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
    let PluginSource::Wasm { module, .. } = &entry.source else {
        return Err("expected WASM source".into());
    };
    assert_eq!(module, std::path::Path::new("plugins/webdav.wasm"));
    std::fs::write(&path, "[vfs.plugins]\nunknown = true\n")?;
    assert!(Registry::for_space("default")?.load_required(path).is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "build the WASI probe and set VFS_LOADER_TEST_TOKEN for the loopback server"]
async fn configured_plugins_open_real_grpc_and_wasm_storage_sessions() -> Result {
    use ah_vfs::{
        application::{dto::InstanceConfig, port::OpenContext},
        domain::{DriverKey, InstanceId},
    };
    use ah_vfs_storage::{
        EntryType, PathExpr, Publication, ReadRange, ReadRequest, StatRequest, SymlinkAction,
        WriteMode, WriteRequest,
    };
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };

    let directory = tempfile::tempdir()?;
    let probe = std::env::var_os("VFS_WASI_TEST_COMPONENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/wasi-probe/guest/wasm32-wasip2/debug/wasi_probe.wasm")
        });
    std::fs::copy(probe, directory.path().join("probe.wasm"))?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let token = std::env::var("VFS_LOADER_TEST_TOKEN")?;
    let backend = Arc::new(vfs_driver_memory::MemoryDriver::new()?);
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        vfs_plugin_grpc_server::serve(listener, backend, &token, async move {
            let _ = stopped.await;
        })
        .await
    });

    let path = directory.path().join("config.toml");
    std::fs::write(
        &path,
        format!(
            r#"
[[vfs.plugins.entries]]
transport = "grpc"
expected_key = "memory"
endpoint = "{endpoint}"
token_env = "VFS_LOADER_TEST_TOKEN"

[[vfs.plugins.entries]]
transport = "wasm"
expected_key = "wasi.probe"
module = "probe.wasm"
"#
        ),
    )?;
    let config = Registry::for_space("default")?.load_required(&path)?;
    let catalog = PluginLoader::load_from(&config, DriverCatalog::new()).await?;
    assert_eq!(catalog.list().len(), 2);
    let root = StatRequest::new(PathExpr::parse("/")?, SymlinkAction::Preserve);
    let wasm = catalog
        .get(&DriverKey::parse("wasi.probe")?)
        .ok_or("Wasm driver")?
        .open(OpenContext::new(
            InstanceId::parse("wasm")?,
            InstanceConfig::default(),
        ))
        .await?;
    assert_eq!(
        wasm.stat_entry(root.clone(), Default::default())
            .await?
            .entry_type(),
        EntryType::Directory
    );

    let grpc = catalog
        .get(&DriverKey::parse("memory")?)
        .ok_or("gRPC driver")?
        .open(OpenContext::new(
            InstanceId::parse("grpc")?,
            InstanceConfig::from([
                ("capacity_size".into(), 64u64.into()),
                ("capacity_unit".into(), "B".into()),
            ]),
        ))
        .await?;
    assert_eq!(
        grpc.stat_entry(root, Default::default())
            .await?
            .entry_type(),
        EntryType::Directory
    );
    let file = PathExpr::parse("/file")?;
    let mut writer = grpc
        .write_file(
            WriteRequest::new(file.clone(), WriteMode::CreateNew, Publication::Staged),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"loaded").await?;
    writer.commit().await?;
    let mut reader = grpc
        .read_file(ReadRequest::new(file, ReadRange::FULL), Default::default())
        .await?;
    drop(grpc);
    assert_eq!(reader.next_chunk().await?, Some(b"loaded".to_vec()));
    assert_eq!(reader.next_chunk().await?, None);

    drop(reader);
    drop(writer);
    drop(wasm);
    drop(catalog);
    let _ = shutdown.send(());
    server.await?.map_err(std::io::Error::other)?;
    Ok(())
}
