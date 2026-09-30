use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{AssetConfig, DatabaseKind};

static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

fn test_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "asset-runtime-config-{}-{}.toml",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn auto_registration_uses_defaults_without_a_file() {
    let config = asset_config::load(test_path())
        .unwrap()
        .get::<AssetConfig>()
        .unwrap();
    assert_eq!(config.database, DatabaseKind::Sqlite);
    assert_eq!(config.root_mount_path, PathBuf::from("/asset-hub-data"));
    assert_eq!(config.config_dir, PathBuf::from("/asset-hub-conf"));
    assert_eq!(
        config.sqlite_path(),
        PathBuf::from("/asset-hub-conf/asset.db")
    );
}

#[test]
fn file_overrides_paths_and_preserves_database_default() {
    let path = test_path();
    std::fs::write(
        &path,
        "[asset]\nroot_mount_path = '/storage'\nconfig_dir = '/custom'\nsqlite_path = '/custom/metadata.db'\n",
    )
    .unwrap();
    let loaded = asset_config::load(&path);
    std::fs::remove_file(path).unwrap();
    let config = loaded.unwrap().get::<AssetConfig>().unwrap();
    assert_eq!(config.database, DatabaseKind::Sqlite);
    assert_eq!(config.root_mount_path, PathBuf::from("/storage"));
    assert_eq!(config.config_dir, PathBuf::from("/custom"));
    // A file path in the configuration cannot override the fixed filename.
    assert_eq!(config.sqlite_path(), PathBuf::from("/custom/asset.db"));
}

#[test]
fn example_config_matches_defaults() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config.example.toml");
    assert!(path.is_file(), "example configuration must exist");
    let config = asset_config::load(path)
        .unwrap()
        .get::<AssetConfig>()
        .unwrap();
    assert_eq!(config, AssetConfig::default());
}

#[test]
fn unsupported_database_is_rejected() {
    let path = test_path();
    std::fs::write(&path, "[asset]\ndatabase = 'unknown'\n").unwrap();
    let result = asset_config::load(&path);
    std::fs::remove_file(path).unwrap();
    assert!(result.is_err());
}
