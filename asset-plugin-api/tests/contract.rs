use asset_plugin_api::protocol::directory::{
    DirectoryPluginActionOutput, PluginDirectoryActionRequest,
};
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
            "wasm": "dist/plugin.wasm",
            "plugin_api": asset_plugin_api::PLUGIN_API_VERSION
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
    let matcher = &manifest.capabilities.kinds[0].detect;
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
fn manifest_accepts_directory_actions_with_target_specific_requirements() {
    let mut value = manifest_document();
    value["capabilities"]["directory_actions"] = json!([{
        "id": "example.plugin.organize",
        "label": "Organize",
        "handler": "organize",
        "applies_to": {"kinds": ["example:collection"]},
        "access": "write",
        "requires": {"children": true, "resources": true},
        "views": ["json"],
        "ui": {"locations": ["directory_toolbar"]}
    }]);
    value["permissions"]["allow"] = json!([
        "resource.read",
        "directory.read",
        "directory.children.list",
        "directory.resources.list",
        "directory.write"
    ]);

    let manifest = canonical_manifest(&value).unwrap();
    let action = manifest.capabilities.directory_actions[0].to_definition(&manifest.runtime);
    assert!(action.requirements().children);
    assert!(action.requirements().resources);
    assert_eq!(action.ui().locations, ["directory_toolbar"]);
}

#[test]
fn directory_request_and_output_have_separate_wire_effects() {
    let request: PluginDirectoryActionRequest = serde_json::from_value(json!({
        "action": "example.plugin.organize",
        "access": "read_write",
        "input": {},
        "directory": {
            "id": "01900000-0000-7000-8000-000000000001",
            "parent_id": "00000000-0000-0000-0000-000000000000",
            "path": "library",
            "name": "library",
            "kind": "example:collection",
            "created_at": "2026-07-28T00:00:00Z",
            "updated_at": "2026-07-28T00:00:00Z"
        },
        "directory_ref": "directory:reference:call-scoped"
    }))
    .unwrap();
    assert_eq!(request.directory.path, "library");

    let output: DirectoryPluginActionOutput = serde_json::from_value(json!({
        "view": "json",
        "data": {"organized": true},
        "effects": [{"type": "create_child", "name": "covers", "kind": "core:directory"}]
    }))
    .unwrap();
    assert_eq!(output.effects.len(), 1);
}

#[test]
fn legacy_manifest_shapes_are_rejected() {
    let mut version_two = manifest_document();
    version_two["manifest_version"] = json!(2);
    assert!(canonical_manifest(&version_two).is_err());

    let mut legacy_capabilities = manifest_document();
    let capabilities = legacy_capabilities["capabilities"].as_object_mut().unwrap();
    let actions = capabilities.remove("actions").unwrap();
    capabilities.insert("resource_actions".to_string(), actions);
    assert!(serde_json::from_value::<PluginManifest>(legacy_capabilities).is_err());

    let mut legacy_permissions = manifest_document();
    legacy_permissions["permissions"] = json!({
        "resource": {"read": true, "write": false},
        "content": {"read": false, "write": false}
    });
    assert!(serde_json::from_value::<PluginManifest>(legacy_permissions).is_err());
}

#[test]
fn request_and_output_wire_shapes_match_the_v04_goldens() {
    assert_golden_round_trip::<PluginActionRequest>(include_str!(
        "fixtures/action-request-inline-v0.4.json"
    ));
    assert_golden_round_trip::<PluginActionRequest>(include_str!(
        "fixtures/action-request-reference-v0.4.json"
    ));
    assert_golden_round_trip::<PluginActionOutput>(include_str!(
        "fixtures/action-output-v0.4.json"
    ));
    assert_golden_round_trip::<PluginActionOutput>(include_str!(
        "fixtures/action-output-download-v0.4.json"
    ));
    assert_golden_round_trip::<PluginActionFailure>(include_str!(
        "fixtures/action-failure-v0.4.json"
    ));
}

#[test]
fn context_specific_encodings_reject_invalid_wire_combinations() {
    let mut request: Value =
        serde_json::from_str(include_str!("fixtures/action-request-inline-v0.4.json")).unwrap();
    request["content"]["encoding"] = json!("handle");
    assert!(serde_json::from_value::<PluginActionRequest>(request).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/action-output-v0.4.json")).unwrap();
    output["effects"][0]["encoding"] = json!("url");
    assert!(serde_json::from_value::<PluginActionOutput>(output).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/action-output-v0.4.json")).unwrap();
    output["effects"][0]["checksum"] = json!({
        "kind": "sha256",
        "value": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    });
    assert!(serde_json::from_value::<PluginActionOutput>(output).is_err());
}
