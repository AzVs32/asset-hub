use super::*;
use asset_plugin_api::PLUGIN_LOCK_FILE_NAME;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn generated_lock_is_accepted_by_shared_verification() {
    let root = test_root("verify", "test.plugin");
    std::fs::create_dir_all(root.join("arbitrary-assets/nested")).unwrap();
    std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
    std::fs::write(root.join("index.html"), b"html").unwrap();
    std::fs::write(root.join("arbitrary-assets/nested/viewer.js"), b"js").unwrap();
    let manifest_path = root.join("manifest.json");
    std::fs::write(&manifest_path, draft_manifest()).unwrap();

    generate_lock(&manifest_path).unwrap();
    verify_manifest(&manifest_path).unwrap();

    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn verification_detects_changes_without_modifying_the_lock() {
    let root = test_root("changed", "test.plugin");
    std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
    let manifest_path = root.join("manifest.json");
    std::fs::write(&manifest_path, draft_manifest()).unwrap();
    generate_lock(&manifest_path).unwrap();
    let lock_path = root.join(PLUGIN_LOCK_FILE_NAME);
    let lock = std::fs::read(&lock_path).unwrap();

    std::fs::write(root.join("plugin.wasm"), b"changed").unwrap();

    assert!(verify_manifest(&manifest_path).is_err());
    assert_eq!(std::fs::read(lock_path).unwrap(), lock);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn generation_refuses_to_replace_an_existing_lock() {
    let root = test_root("existing", "test.plugin");
    std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
    let manifest_path = root.join("manifest.json");
    std::fs::write(&manifest_path, draft_manifest()).unwrap();
    generate_lock(&manifest_path).unwrap();
    let lock_path = root.join(PLUGIN_LOCK_FILE_NAME);
    let lock = std::fs::read(&lock_path).unwrap();

    assert!(generate_lock(&manifest_path).is_err());
    assert_eq!(std::fs::read(lock_path).unwrap(), lock);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
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
