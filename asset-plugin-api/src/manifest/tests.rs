//! 完整 Manifest 文档的跨字段校验测试。

use super::*;

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
            "wasm": "dist/plugin.wasm",
            "plugin_api": PLUGIN_API_VERSION
        },
        "capabilities": {
            "actions": [{
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
fn manifest_requires_current_manifest_and_plugin_api_versions() {
    let mut document = manifest_document();
    document["manifest_version"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<PluginManifest>(document.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    document["manifest_version"] = serde_json::json!(MANIFEST_VERSION);
    document["runtime"]["plugin_api"] = serde_json::json!("asset-hub.plugin-api@0.1");
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
fn manifest_rejects_unknown_fields_at_every_level() {
    let mut document = manifest_document();
    document["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());

    let mut document = manifest_document();
    document["capabilities"]["actions"][0]["applies_to"]["typo"] = serde_json::json!([]);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());

    let mut document = manifest_document();
    document["runtime"]["wais"] = serde_json::json!(false);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());
}

#[test]
fn manifest_accepts_download_view_and_rejects_removed_binary_url_view() {
    let mut document = manifest_document();
    document["capabilities"]["actions"][0]["views"] = serde_json::json!(["download"]);
    serde_json::from_value::<PluginManifest>(document.clone())
        .unwrap()
        .validate()
        .unwrap();

    document["capabilities"]["actions"][0]["views"] = serde_json::json!(["binary_url"]);
    let error = serde_json::from_value::<PluginManifest>(document)
        .unwrap()
        .validate()
        .unwrap_err();
    assert!(error.contains("unsupported view `binary_url`"));
}

#[test]
fn manifest_rejects_removed_table_and_form_views() {
    for view in ["table", "form"] {
        let mut document = manifest_document();
        document["capabilities"]["actions"][0]["views"] = serde_json::json!([view]);
        let error = serde_json::from_value::<PluginManifest>(document)
            .unwrap()
            .validate()
            .unwrap_err();
        assert!(error.contains(&format!("unsupported view `{view}`")));
    }
}
