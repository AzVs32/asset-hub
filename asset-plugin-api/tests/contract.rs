use asset_plugin_api::abi::DirectoryPageRequest;
use asset_plugin_api::manifest::{
    MANIFEST_VERSION, PluginManifestDocument, ValidatedPluginManifest,
};
use asset_plugin_api::protocol::{
    PLUGIN_API_VERSION, PluginActionFailure, PluginResourceActionOutput,
    PluginResourceActionRequest,
};
use asset_plugin_api::spec;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::path::Path;

fn assert_golden_round_trip<T>(source: &str)
where
    T: DeserializeOwned + Serialize,
{
    let expected: Value = serde_json::from_str(source).unwrap();
    let parsed: T = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), expected);
}

fn canonical_manifest(value: &Value) -> Result<ValidatedPluginManifest, String> {
    let manifest: PluginManifestDocument = serde_json::from_value(value.clone())
        .map_err(|error| format!("Serde rejected manifest: {error}"))?;
    let manifest = manifest
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
            "plugin_api": PLUGIN_API_VERSION
        },
        "capabilities": {
            "resource_actions": [{
                "id": "example.plugin.action",
                "label": "Example Action",
                "handler": "run",
                "applies_to": {"kinds": ["core:resource"]},
                "output": {"views": ["json"]}
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
            value["capabilities"]["resource_actions"][0]["output"]["views"] =
                json!(["json", "json"]);
            value
        },
    ];

    for value in invalid_documents {
        let manifest: PluginManifestDocument = serde_json::from_value(value).unwrap();
        assert!(manifest.validate().is_err());
    }
}

#[test]
fn schema_hint_is_not_part_of_the_manifest_contract() {
    let mut value = manifest_document();
    value["$schema"] = json!("https://example.invalid/plugin-manifest.json");

    assert!(serde_json::from_value::<PluginManifestDocument>(value).is_err());
}

#[test]
fn manifest_matchers_are_normalized_by_serde() {
    let mut value = manifest_document();
    value["capabilities"]["resource_kinds"] = json!([{
        "kind": "example:markdown",
        "parent": "core:resource",
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
fn resource_and_directory_action_ids_use_separate_namespaces() {
    let mut value = manifest_document();
    value["capabilities"]["directory_actions"] = json!([{
        "id": "example.plugin.action",
        "label": "Directory Action",
        "handler": "run_for_directory",
        "output": {"views": ["json"]}
    }]);
    value["permissions"]["allow"] = json!(["resource.read", "directory.read"]);

    canonical_manifest(&value).unwrap();
}

#[test]
fn request_and_output_wire_shapes_match_the_current_goldens() {
    assert_golden_round_trip::<PluginManifestDocument>(include_str!("fixtures/manifest-v5.json"));
    assert_golden_round_trip::<PluginResourceActionRequest>(include_str!(
        "fixtures/resource-action-request-inline-v4.json"
    ));
    assert_golden_round_trip::<PluginResourceActionRequest>(include_str!(
        "fixtures/resource-action-request-reference-v4.json"
    ));
    assert_golden_round_trip::<PluginResourceActionOutput>(include_str!(
        "fixtures/resource-action-output-v4.json"
    ));
    assert_golden_round_trip::<PluginResourceActionOutput>(include_str!(
        "fixtures/resource-action-output-download-v4.json"
    ));
    assert_golden_round_trip::<PluginActionFailure>(include_str!(
        "fixtures/action-failure-v4.json"
    ));
    assert_golden_round_trip::<DirectoryPageRequest>(include_str!(
        "fixtures/directory-page-request-v4.json"
    ));
}

#[test]
fn committed_language_neutral_specs_are_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("spec");
    let expected: Value =
        serde_json::from_slice(&std::fs::read(root.join("contract-v4.json")).unwrap()).unwrap();
    assert_eq!(
        expected,
        serde_json::to_value(spec::contract_catalog()).unwrap()
    );
    for (name, schema) in spec::schemas() {
        let expected: Value =
            serde_json::from_slice(&std::fs::read(root.join(name)).unwrap()).unwrap();
        assert_eq!(expected, serde_json::to_value(schema).unwrap(), "{name}");
    }
    let typescript = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sdk/asset-web-sdk/src/contract.generated.ts"),
    )
    .unwrap();
    assert_eq!(typescript, spec::typescript_catalog_module());
}

#[test]
fn context_specific_encodings_reject_invalid_wire_combinations() {
    let mut request: Value = serde_json::from_str(include_str!(
        "fixtures/resource-action-request-inline-v4.json"
    ))
    .unwrap();
    request["content"]["encoding"] = json!("handle");
    assert!(serde_json::from_value::<PluginResourceActionRequest>(request).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/resource-action-output-v4.json")).unwrap();
    output["effects"][0]["encoding"] = json!("url");
    assert!(serde_json::from_value::<PluginResourceActionOutput>(output).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/resource-action-output-v4.json")).unwrap();
    output["effects"][0]["checksum"] = json!({
        "kind": "sha256",
        "value": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    });
    assert!(serde_json::from_value::<PluginResourceActionOutput>(output).is_err());
}

#[test]
fn all_versioned_wire_documents_reject_unknown_fields() {
    let mut request: Value = serde_json::from_str(include_str!(
        "fixtures/resource-action-request-inline-v4.json"
    ))
    .unwrap();
    request["unexpected"] = json!(true);
    assert!(serde_json::from_value::<PluginResourceActionRequest>(request).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/resource-action-output-v4.json")).unwrap();
    output["view"]["unexpected"] = json!(true);
    assert!(serde_json::from_value::<PluginResourceActionOutput>(output).is_err());
}

#[test]
fn resource_requests_reject_removed_v3_state_fields() {
    let request: Value = serde_json::from_str(include_str!(
        "fixtures/resource-action-request-inline-v4.json"
    ))
    .unwrap();

    let mut deleted_at = request.clone();
    deleted_at["resource"]["deleted_at"] = json!("2026-07-16T10:05:00Z");
    assert!(serde_json::from_value::<PluginResourceActionRequest>(deleted_at).is_err());

    let mut verification_status = request;
    verification_status["resource"]["content"]["verification_status"] = json!("verified");
    assert!(serde_json::from_value::<PluginResourceActionRequest>(verification_status).is_err());
}
