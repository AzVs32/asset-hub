use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn verify_accepts_a_startup_generated_lock() {
    let root = test_root("verify", "test.plugin");
    std::fs::create_dir_all(root.join("arbitrary-assets/nested")).unwrap();
    std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
    std::fs::write(root.join("index.html"), b"html").unwrap();
    std::fs::write(root.join("arbitrary-assets/nested/viewer.js"), b"js").unwrap();
    let manifest_path = root.join("manifest.json");
    std::fs::write(&manifest_path, draft_manifest()).unwrap();
    write_test_lock(&root);

    verify_manifest(&manifest_path).unwrap();

    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn verify_detects_artifact_changes_without_modifying_the_lock() {
    let root = test_root("changed", "test.plugin");
    std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
    let manifest_path = root.join("manifest.json");
    std::fs::write(&manifest_path, draft_manifest()).unwrap();
    write_test_lock(&root);
    let lock = std::fs::read(root.join(PLUGIN_LOCK_FILE_NAME)).unwrap();

    std::fs::write(root.join("plugin.wasm"), b"changed").unwrap();

    assert!(verify_manifest(&manifest_path).is_err());
    assert_eq!(std::fs::read(root.join(PLUGIN_LOCK_FILE_NAME)).unwrap(), lock);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

fn write_test_lock(root: &Path) {
    let lock = PluginManifestLock {
        manifest_version: 1,
        plugin_id: "test.plugin".to_string(),
        integrity: package_integrity(root).unwrap(),
    };
    std::fs::write(
        root.join(PLUGIN_LOCK_FILE_NAME),
        serde_json::to_vec_pretty(&lock).unwrap(),
    )
    .unwrap();
}

fn draft_manifest() -> &'static str {
    r#"{
      "manifest_version": 1,
      "plugin": {"id": "test.plugin", "name": "Test", "version": "0.1.0", "publisher": "test"},
      "runtime": {"type": "extism", "plugin_api": "asset-hub.plugin-api@1"},
      "capabilities": {"kinds": [], "resource_actions": []},
      "permissions": {
        "allow": ["resource.read"],
        "network": false,
        "filesystem": false
      }
    }"#
}

fn test_root(name: &str, plugin_id: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "asset-plugin-{name}-{}-{nanos}",
        std::process::id()
    ));
    let package = root.join(plugin_id);
    std::fs::create_dir_all(&package).unwrap();
    package
}
