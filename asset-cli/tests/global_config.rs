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
