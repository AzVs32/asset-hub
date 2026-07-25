use asset_plugin_api::{
    MANIFEST_VERSION, PluginActionFailure, PluginActionOutput, PluginActionRequest, PluginManifest,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn assert_golden_round_trip<T>(source: &str)
where
    T: DeserializeOwned + Serialize,
{
    let expected: Value = serde_json::from_str(source).unwrap();
    let parsed: T = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), expected);
}

fn canonical_manifest(value: &Value) -> Result<PluginManifest, String> {
    let manifest: PluginManifest = serde_json::from_value(value.clone())
        .map_err(|error| format!("Serde rejected manifest: {error}"))?;
    manifest
        .validate()
        .map_err(|error| format!("host rejected manifest: {error}"))?;
    Ok(manifest)
}

fn manifest_document() -> Value {
    json!({
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
fn host_rejects_canonical_manifest_violations() {
    let template = manifest_document();
    let invalid_documents = [
        {
            let mut value = template.clone();
            value["plugin"]["id"] = json!("example:plugin");
            value
        },
        {
            let mut value = template.clone();
            value["plugin"]["publisher"] = json!("   ");
            value
        },
        {
            let mut value = template.clone();
            value["capabilities"]["actions"][0]["views"] = json!(["json", "json"]);
            value
        },
    ];

    for value in invalid_documents {
        let manifest: PluginManifest = serde_json::from_value(value).unwrap();
        assert!(manifest.validate().is_err());
    }
}

#[test]
fn manifest_matchers_are_normalized_by_serde() {
    let mut value = manifest_document();
    value["capabilities"]["kinds"] = json!([{
        "kind": "example:markdown",
        "parent": "core:document",
        "label": "Markdown",
        "detect": {
            "mime_types": [" Text/Markdown "],
            "extensions": ["MD"]
        }
    }]);

    let manifest = canonical_manifest(&value).unwrap();
    let matcher = &manifest.capabilities.resource_kinds[0].detect;
    assert_eq!(matcher.mime_types(), ["text/markdown"]);
    assert_eq!(matcher.extensions(), [".md"]);
}

#[test]
fn manifest_accepts_extensible_directory_kind_hierarchies() {
    let mut value = manifest_document();
    value["capabilities"]["directory_kinds"] = json!([{
        "kind": "example:collection",
        "parent": "core:directory",
        "label": "Collection"
    }]);

    let manifest = canonical_manifest(&value).unwrap();
    let kind = &manifest.capabilities.directory_kinds[0];
    assert_eq!(kind.kind, "example:collection");
    assert_eq!(kind.parent.as_deref(), Some("core:directory"));
}

#[test]
fn v2_compatibility_is_runtime_only_and_serializes_to_the_canonical_shape() {
    let mut value = manifest_document();
    value["manifest_version"] = json!(2);
    value["runtime"]["plugin_api"] = json!(asset_plugin_api::PLUGIN_API_VERSION);
    let capabilities = value["capabilities"].as_object_mut().unwrap();
    let actions = capabilities.remove("actions").unwrap();
    capabilities.insert("resource_actions".to_string(), actions);
    value["permissions"] = json!({
        "resource": {"read": true, "write": false},
        "content": {"read": false, "write": false},
        "network": false,
        "filesystem": false
    });

    let manifest: PluginManifest = serde_json::from_value(value).unwrap();
    manifest.validate().unwrap();
    let normalized = serde_json::to_value(manifest).unwrap();
    assert!(normalized["capabilities"].get("actions").is_some());
    assert!(normalized["capabilities"].get("resource_actions").is_none());
    assert!(normalized["permissions"].get("allow").is_some());
    assert!(normalized["permissions"].get("resource").is_none());
}

#[test]
fn request_and_output_wire_shapes_match_the_v03_goldens() {
    assert_golden_round_trip::<PluginActionRequest>(include_str!(
        "fixtures/action-request-inline-v0.3.json"
    ));
    assert_golden_round_trip::<PluginActionRequest>(include_str!(
        "fixtures/action-request-reference-v0.3.json"
    ));
    assert_golden_round_trip::<PluginActionOutput>(include_str!(
        "fixtures/action-output-v0.3.json"
    ));
    assert_golden_round_trip::<PluginActionFailure>(include_str!(
        "fixtures/action-failure-v0.3.json"
    ));
}

#[test]
fn context_specific_encodings_reject_invalid_wire_combinations() {
    let mut request: Value =
        serde_json::from_str(include_str!("fixtures/action-request-inline-v0.3.json")).unwrap();
    request["content"]["encoding"] = json!("handle");
    assert!(serde_json::from_value::<PluginActionRequest>(request).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/action-output-v0.3.json")).unwrap();
    output["effects"][0]["encoding"] = json!("url");
    assert!(serde_json::from_value::<PluginActionOutput>(output).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/action-output-v0.3.json")).unwrap();
    output["effects"][0]["checksum"] = json!({
        "kind": "sha256",
        "value": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    });
    assert!(serde_json::from_value::<PluginActionOutput>(output).is_err());
}
