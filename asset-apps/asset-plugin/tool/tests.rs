use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn seal_generates_wasm_and_web_integrity() {
    let root = test_root("seal");
    std::fs::create_dir_all(root.join("web/nested")).unwrap();
    std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
    std::fs::write(root.join("web/index.html"), b"html").unwrap();
    std::fs::write(root.join("web/nested/viewer.js"), b"js").unwrap();
    let manifest_path = root.join("plugin.json");
    std::fs::write(&manifest_path, draft_manifest()).unwrap();

    let sealed = seal_manifest(&manifest_path).unwrap();
    verify_manifest(&manifest_path).unwrap();
    seal_manifest(&manifest_path).unwrap();
    verify_manifest(&manifest_path).unwrap();

    let PluginRuntime::Extism { .. } = sealed.runtime else {
        panic!("expected Extism runtime");
    };
    assert!(sealed.web.is_some());
    let document: serde_json::Value = read_json(&manifest_path).unwrap();
    let lock: PluginManifestLock = read_json(&manifest_lock_path(&manifest_path)).unwrap();
    assert_eq!(
        lock.runtime.unwrap().wasm_sha256,
        digest_file(&root.join("plugin.wasm")).unwrap()
    );
    assert_eq!(lock.web.unwrap().integrity.len(), 2);
    assert!(document["runtime"].get("wasm_sha256").is_none());
    assert!(document["web"].get("integrity").is_none());
    assert!(document["runtime"].get("wasi").is_none());
    assert!(document["plugin"].get("description").is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn generate_writes_a_seal_ready_manifest_without_generated_hashes() {
    let root = test_root("generate");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("manifest.json");

    let generated = generate_manifest(&path).unwrap();
    let document: serde_json::Value = read_json(&path).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), MANIFEST_TEMPLATE);
    assert_eq!(generated.plugin_id(), "example.plugin");
    assert_eq!(
        document["manifest_version"],
        asset_plugin_api::MANIFEST_VERSION
    );
    assert_eq!(document["runtime"]["wasm"], "dist/plugin.wasm");
    assert!(document["runtime"].get("wasm_sha256").is_none());
    assert_eq!(document["capabilities"]["actions"][0]["handler"], "run");
    assert!(generate_manifest(&path).is_err());
    std::fs::create_dir_all(root.join("dist")).unwrap();
    std::fs::write(root.join("dist/plugin.wasm"), b"wasm").unwrap();
    seal_manifest(&path).unwrap();
    verify_manifest(&path).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn generate_schema_writes_the_embedded_schema_without_overwriting() {
    let root = test_root("schema");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("manifest.schema.json");
    generate_schema(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), MANIFEST_SCHEMA);
    assert!(generate_schema(&path).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn verify_detects_artifact_changes_without_modifying_manifest() {
    let root = test_root("verify");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
    let manifest_path = root.join("plugin.json");
    std::fs::write(&manifest_path, draft_manifest_without_web()).unwrap();
    seal_manifest(&manifest_path).unwrap();
    let sealed = std::fs::read(&manifest_path).unwrap();

    std::fs::write(root.join("plugin.wasm"), b"changed").unwrap();
    assert!(verify_manifest(&manifest_path).is_err());
    assert_eq!(std::fs::read(&manifest_path).unwrap(), sealed);
    let _ = std::fs::remove_dir_all(root);
}

fn draft_manifest() -> String {
    draft_manifest_with_web(r#", "web": {"root": "web"}"#)
}

fn draft_manifest_without_web() -> String {
    draft_manifest_with_web("")
}

fn draft_manifest_with_web(web: &str) -> String {
    format!(
        r#"{{
          "manifest_version": 2,
          "plugin": {{"id": "test.plugin", "name": "Test", "version": "0.1.0", "publisher": "test"}},
          "runtime": {{"type": "extism", "wasm": "plugin.wasm", "plugin_api": "asset-hub.plugin-api@0.3"}},
          "capabilities": {{"resource_kinds": [], "resource_actions": []}},
          "permissions": {{
            "resource": {{"read": true, "write": false}},
            "content": {{"read": false, "write": false}},
            "network": false,
            "filesystem": false
          }}{web}
        }}"#
    )
}

fn test_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "asset-plugin-{name}-{}-{nanos}",
        std::process::id()
    ))
}
