use asset_plugin_api::{
    MANIFEST_SCHEMA, MANIFEST_TEMPLATE, PluginActionFailure, PluginActionOutput,
    PluginActionRequest, PluginManifest,
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
    let schema: Value = serde_json::from_str(MANIFEST_SCHEMA).unwrap();
    jsonschema::draft202012::validate(&schema, value).map_err(|error| error.to_string())?;
    let manifest: PluginManifest = serde_json::from_value(value.clone())
        .map_err(|error| format!("Serde rejected manifest: {error}"))?;
    manifest
        .validate()
        .map_err(|error| format!("host rejected manifest: {error}"))?;
    Ok(manifest)
}

#[test]
fn manifest_schema_is_valid_and_template_passes_all_contract_layers() {
    let schema: Value = serde_json::from_str(MANIFEST_SCHEMA).unwrap();
    jsonschema::draft202012::meta::validate(&schema).unwrap();

    let template: Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
    canonical_manifest(&template).unwrap();
}

#[test]
fn schema_and_host_reject_the_same_canonical_manifest_violations() {
    let schema: Value = serde_json::from_str(MANIFEST_SCHEMA).unwrap();
    let template: Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
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
        assert!(!jsonschema::draft202012::is_valid(&schema, &value));
        let manifest: PluginManifest = serde_json::from_value(value).unwrap();
        assert!(manifest.validate().is_err());
    }
}

#[test]
fn manifest_matchers_are_schema_valid_and_normalized_by_serde() {
    let mut value: Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
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
fn kind_metadata_capability_passes_manifest_and_json_schema_contracts() {
    let mut value: Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
    value["capabilities"]["kinds"] = json!([{
        "kind": "example:image",
        "parent": "core:image",
        "metadata": {
            "schema_version": 1,
            "schema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "readOnly": true,
                "additionalProperties": false,
                "properties": {
                    "quality": {"type": "integer", "minimum": 0, "maximum": 100}
                }
            }
        }
    }]);

    let manifest = canonical_manifest(&value).unwrap();
    let metadata = manifest.capabilities.resource_kinds[0]
        .metadata
        .as_ref()
        .unwrap();
    assert_eq!(metadata.schema_version, 1);
    jsonschema::draft202012::meta::validate(&metadata.schema).unwrap();
}

#[test]
fn bundled_kind_metadata_schemas_are_valid_read_only_draft_2020_12_objects() {
    let manifests = [
        include_str!("../../asset-infra/src/official_plugins/core_image/manifest.json"),
        include_str!("../../asset-infra/src/official_plugins/core_document/manifest.json"),
        include_str!("../../asset-infra/src/official_plugins/core_video/manifest.json"),
        include_str!("../../plugins/azvs-markdown/manifest.json"),
        include_str!("../../plugins/azvs-epub/manifest.json"),
    ];

    for source in manifests {
        let value: Value = serde_json::from_str(source).unwrap();
        let manifest = canonical_manifest(&value).unwrap();
        for kind in manifest.capabilities.resource_kinds {
            let metadata = kind.metadata.expect("bundled kind must declare metadata");
            jsonschema::draft202012::meta::validate(&metadata.schema).unwrap();
            assert_eq!(metadata.schema["readOnly"], true);
            assert_eq!(metadata.schema["additionalProperties"], false);
            assert!(metadata.schema.get("required").is_none());
        }
    }
}

#[test]
fn v2_compatibility_is_runtime_only_and_serializes_to_the_canonical_shape() {
    let mut value: Value = serde_json::from_str(MANIFEST_TEMPLATE).unwrap();
    value.as_object_mut().unwrap().remove("$schema");
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
fn request_and_output_wire_shapes_match_the_v02_goldens() {
    assert_golden_round_trip::<PluginActionRequest>(include_str!(
        "fixtures/action-request-inline-v0.2.json"
    ));
    assert_golden_round_trip::<PluginActionRequest>(include_str!(
        "fixtures/action-request-reference-v0.2.json"
    ));
    assert_golden_round_trip::<PluginActionOutput>(include_str!(
        "fixtures/action-output-v0.2.json"
    ));
    assert_golden_round_trip::<PluginActionFailure>(include_str!(
        "fixtures/action-failure-v0.2.json"
    ));
}

#[test]
fn context_specific_encodings_reject_invalid_wire_combinations() {
    let mut request: Value =
        serde_json::from_str(include_str!("fixtures/action-request-inline-v0.2.json")).unwrap();
    request["content"]["encoding"] = json!("handle");
    assert!(serde_json::from_value::<PluginActionRequest>(request).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/action-output-v0.2.json")).unwrap();
    output["effects"][0]["encoding"] = json!("url");
    assert!(serde_json::from_value::<PluginActionOutput>(output).is_err());
}
