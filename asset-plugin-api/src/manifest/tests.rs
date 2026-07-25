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
            "wasm": "dist/plugin.wasm"
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
fn compatibility_window_accepts_v2_with_current_api_and_rejects_unknown_versions() {
    let mut document = manifest_document();
    document["manifest_version"] = serde_json::json!(2);
    document["runtime"]["plugin_api"] = serde_json::json!(PLUGIN_API_VERSION);
    let capabilities = document["capabilities"].as_object_mut().unwrap();
    let actions = capabilities.remove("actions").unwrap();
    capabilities.insert("resource_actions".to_string(), actions);
    document["permissions"] = serde_json::json!({
        "resource": {"read": true, "write": false},
        "content": {"read": false, "write": false},
        "network": false,
        "filesystem": false
    });
    let manifest: PluginManifest = serde_json::from_value(document.clone()).unwrap();
    manifest.validate().unwrap();

    document["manifest_version"] = serde_json::json!(1);
    assert!(
        serde_json::from_value::<PluginManifest>(document.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    document["manifest_version"] = serde_json::json!(2);
    document["runtime"]["plugin_api"] = serde_json::json!("asset-hub.plugin-api@0.1");
    assert!(
        serde_json::from_value::<PluginManifest>(document)
            .unwrap()
            .validate()
            .is_err()
    );
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
