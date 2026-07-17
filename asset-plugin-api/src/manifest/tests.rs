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

#[test]
fn manifest_rejects_invalid_kind_metadata_envelopes_and_schema_roots() {
    let valid_metadata = serde_json::json!({
        "schema_version": 1,
        "schema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "readOnly": true,
            "additionalProperties": false,
            "properties": {}
        }
    });

    let invalid_metadata = [
        {
            let mut value = valid_metadata.clone();
            value["schema_version"] = serde_json::json!(0);
            value
        },
        {
            let mut value = valid_metadata.clone();
            value["schema"]["$schema"] =
                serde_json::json!("http://json-schema.org/draft-07/schema#");
            value
        },
        {
            let mut value = valid_metadata.clone();
            value["schema"]["type"] = serde_json::json!("array");
            value
        },
        {
            let mut value = valid_metadata.clone();
            value["schema"]["readOnly"] = serde_json::json!("yes");
            value
        },
        {
            let mut value = valid_metadata.clone();
            value["schema"]["additionalProperties"] = serde_json::json!(true);
            value
        },
        {
            let mut value = valid_metadata;
            value["schema"]["properties"] = serde_json::json!({
                "unsafe": {"$ref": "https://example.com/remote-schema.json"}
            });
            value
        },
    ];

    for metadata in invalid_metadata {
        let mut document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
        document["capabilities"]["kinds"] = serde_json::json!([{
            "kind": "example:document",
            "metadata": metadata
        }]);
        let manifest: PluginManifest = serde_json::from_value(document).unwrap();
        assert!(manifest.validate().is_err());
    }
}
