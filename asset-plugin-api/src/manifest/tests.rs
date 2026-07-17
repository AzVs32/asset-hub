use super::*;

#[test]
fn embedded_manifest_template_is_a_v3_draft_without_generated_integrity() {
    let document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();

    assert_eq!(document["manifest_version"], MANIFEST_VERSION);
    assert!(document["runtime"].get("plugin_api").is_none());
    assert!(document["runtime"].get("wasm_sha256").is_none());
    assert!(document.get("web").is_none());
}

#[test]
fn embedded_json_schema_is_draft_2020_12() {
    let schema: serde_json::Value = serde_json::from_str(MANIFEST_SCHEMA).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(
        schema["properties"]["manifest_version"]["const"],
        MANIFEST_VERSION
    );
}

#[test]
fn compatibility_window_accepts_v2_with_current_api_and_rejects_unknown_versions() {
    let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
    document.as_object_mut().unwrap().remove("$schema");
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
    let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
    document["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());

    let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
    document["capabilities"]["actions"][0]["applies_to"]["typo"] = serde_json::json!([]);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());

    let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
    document["runtime"]["wais"] = serde_json::json!(false);
    assert!(serde_json::from_value::<PluginManifest>(document).is_err());
}
