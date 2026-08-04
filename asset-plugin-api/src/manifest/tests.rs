//! 完整 Manifest 文档的跨字段校验测试。

use super::*;
use crate::protocol::PLUGIN_API_VERSION;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn manifest_document() -> serde_json::Value {
    serde_json::json!({
        "manifest_version": MANIFEST_VERSION,
        "plugin": {
            "id": "example.plugin",
            "name": "Example Plugin",
            "version": "0.1.0",
            "publisher": "example"
        },
        "runtime": {
            "type": "extism",
            "plugin_api": PLUGIN_API_VERSION
        },
        "capabilities": {
            "resource_actions": [{
                "id": "example.plugin.action",
                "label": "Example Action",
                "handler": "run",
                "applies_to": {"kinds": ["core:resource"]},
                "views": ["json"]
            }]
        },
        "permissions": {"allow": ["resource.read"]}
    })
}

#[test]
fn manifest_requires_current_versions() {
    let mut document = manifest_document();
    document["manifest_version"] = serde_json::json!(MANIFEST_VERSION + 1);
    assert!(
        serde_json::from_value::<PluginManifest>(document.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    document["manifest_version"] = serde_json::json!(MANIFEST_VERSION);
    document["runtime"]["plugin_api"] = serde_json::json!("asset-hub.plugin-api@2");
    assert!(
        serde_json::from_value::<PluginManifest>(document.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    document["runtime"]
        .as_object_mut()
        .unwrap()
        .remove("plugin_api");
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());
}

#[test]
fn manifest_rejects_host_owned_builtin_runtime() {
    let mut document = manifest_document();
    document["runtime"] = serde_json::json!({"type": "builtin"});

    assert!(serde_json::from_value::<PluginManifest>(document).is_err());
}

#[test]
fn lock_uses_one_flat_integrity_map() {
    let manifest: PluginManifest = serde_json::from_value(manifest_document()).unwrap();
    let digest = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let lock = PluginManifestLock {
        manifest_version: MANIFEST_VERSION,
        plugin_id: manifest.plugin_id().to_string(),
        integrity: BTreeMap::from([
            (PathBuf::from(PLUGIN_WASM_FILE_NAME), digest.to_string()),
            (
                PathBuf::from(PLUGIN_WEB_ENTRY_FILE_NAME),
                digest.to_string(),
            ),
            (PathBuf::from("assets/app.js"), digest.to_string()),
        ]),
    };

    lock.validate_for(&manifest).unwrap();
    let document = serde_json::to_value(lock).unwrap();
    assert!(document.get("integrity").is_some());
    assert!(document.get("runtime").is_none());
    assert!(document.get("web").is_none());
}

#[test]
fn lock_rejects_the_removed_runtime_and_web_groups() {
    let old_lock = serde_json::json!({
        "manifest_version": MANIFEST_VERSION,
        "plugin_id": "example.plugin",
        "integrity": {},
        "runtime": {"wasm_sha256": "unused"},
        "web": {"integrity": {}}
    });

    assert!(serde_json::from_value::<PluginManifestLock>(old_lock).is_err());
}

#[test]
fn manifest_rejects_unknown_fields_at_every_level() {
    let mut document = manifest_document();
    document["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());

    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]["applies_to"]["typo"] = serde_json::json!([]);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());

    let mut document = manifest_document();
    document["runtime"]["wais"] = serde_json::json!(false);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());

    let mut document = manifest_document();
    document["runtime"]["wasm"] = serde_json::json!("custom.wasm");
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());

    let mut document = manifest_document();
    document["web"] = serde_json::json!({"root": "dist"});
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());
}

#[test]
fn manifest_validates_provided_capability_ids() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]["provides"] =
        serde_json::json!("Resource.Thumbnail");

    assert!(
        serde_json::from_value::<PluginManifest>(document)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn manifest_accepts_download_view() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]["views"] = serde_json::json!(["download"]);
    serde_json::from_value::<PluginManifest>(document)
        .unwrap()
        .validate()
        .unwrap();
}
