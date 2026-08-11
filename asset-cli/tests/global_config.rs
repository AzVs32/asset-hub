use asset_infra::config::{
    AssetInfraConfig, BlobConfig, DatabaseConfig, LocalBlobConfig, LocalBlobSyncConfig,
    SqliteDatabaseConfig,
};
use std::path::{Path, PathBuf};
use std::process::Command;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("asset-cli-config-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn user_commands_honor_the_global_config_argument() {
    let root = TestRoot::new();
    let working_directory = root.path().join("working");
    std::fs::create_dir_all(&working_directory).unwrap();
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite: SqliteDatabaseConfig { max_connections: 1 },
            ..DatabaseConfig::default()
        },
        blob: BlobConfig {
            local: LocalBlobConfig {
                root: root.path().join("configured-data"),
                sync: LocalBlobSyncConfig {
                    enabled: false,
                    ..LocalBlobSyncConfig::default()
                },
            },
            ..BlobConfig::default()
        },
        ..AssetInfraConfig::default()
    };
    let config_path = root.path().join("asset-hub.toml");
    std::fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_asset"))
        .current_dir(&working_directory)
        .arg("--config")
        .arg(&config_path)
        .args(["user", "--list"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "asset failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(config.sqlite_path().is_file());
    assert!(
        !working_directory
            .join("data/.asset-hub/asset-hub.sqlite")
            .exists()
    );
}

#[test]
fn plugin_commands_resolve_plugin_ids_from_the_configured_data_root() {
    let root = TestRoot::new();
    let configured_data = root.path().join("configured-data");
    let package = configured_data
        .join(".asset-hub/plugins")
        .join("example.plugin");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("manifest.json"),
        r#"{
          "manifest_version": 2,
          "plugin": {
            "id": "example.plugin",
            "name": "Example",
            "version": "0.1.0",
            "publisher": "test"
          },
          "runtime": {
            "type": "extism",
            "wasi": false,
            "plugin_api": "asset-hub.plugin-api@3"
          },
          "capabilities": {
            "resource_kinds": [],
            "resource_actions": []
          },
          "permissions": { "allow": ["resource.read"] }
        }"#,
    )
    .unwrap();
    std::fs::write(package.join("plugin.wasm"), b"\0asm").unwrap();

    let config = AssetInfraConfig {
        blob: BlobConfig {
            local: LocalBlobConfig {
                root: configured_data.clone(),
                sync: LocalBlobSyncConfig {
                    enabled: false,
                    ..LocalBlobSyncConfig::default()
                },
            },
            ..BlobConfig::default()
        },
        ..AssetInfraConfig::default()
    };
    let config_path = root.path().join("asset-hub.toml");
    std::fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    for operation in ["--seal", "--verify"] {
        let output = Command::new(env!("CARGO_BIN_EXE_asset"))
            .arg("--config")
            .arg(&config_path)
            .args(["plugin", operation, "example.plugin"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "asset {operation} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert!(package.join("manifest.lock.json").is_file());
    assert!(!configured_data.join(".asset-hub/asset-hub.sqlite").exists());
}
