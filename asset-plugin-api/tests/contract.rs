use asset_plugin_api::manifest::{MANIFEST_VERSION, PluginManifest};
use asset_plugin_api::protocol::directory::{
    PluginDirectoryActionOutput, PluginDirectoryActionRequest,
};
use asset_plugin_api::protocol::{
    PLUGIN_API_VERSION, PluginActionFailure, PluginResourceActionOutput,
    PluginResourceActionRequest,
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
        let manifest: PluginManifest = serde_json::from_value(value).unwrap();
        assert!(manifest.validate().is_err());
    }
}

#[test]
fn manifest_matchers_are_normalized_by_serde() {
    let mut value = manifest_document();
    value["capabilities"]["resource_kinds"] = json!([{
        "kind": "example:markdown",
        "parent": "core:text",
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
fn manifest_accepts_directory_actions_with_target_specific_requirements() {
    let mut value = manifest_document();
    value["capabilities"]["directory_actions"] = json!([{
        "id": "example.plugin.organize",
        "label": "Organize",
        "handler": "organize",
        "applies_to": {"kinds": ["example:collection"]},
        "access": "write",
        "requires": {"children": true, "resources": true},
        "output": {"views": ["json"]},
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
    let action = &manifest.capabilities.directory_actions[0];
    let requirements = action.requires.as_ref().unwrap();
    assert!(requirements.children);
    assert!(requirements.resources);
    assert_eq!(action.ui.as_ref().unwrap().locations, ["directory_toolbar"]);
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
fn manifest_actions_can_provide_singleton_host_capabilities() {
    let mut value = manifest_document();
    value["capabilities"]["resource_actions"][0]["provides"] = json!("thumbnail");
    value["capabilities"]["resource_actions"][0]["output"]["views"] = json!(["media"]);
    value["capabilities"]["resource_actions"][0]["ui"] =
        json!({"locations": ["resource_list_thumbnail"]});
    value["capabilities"]["directory_actions"] = json!([{
        "id": "example.plugin.directory-thumbnail",
        "provides": "thumbnail",
        "label": "Directory Thumbnail",
        "handler": "directory_thumbnail",
        "output": {"views": ["media"]},
        "ui": {"locations": ["directory_list_thumbnail"]}
    }]);
    value["permissions"]["allow"] = json!(["resource.read", "directory.read"]);

    let manifest = canonical_manifest(&value).unwrap();

    assert_eq!(
        manifest.capabilities.resource_actions[0]
            .provides
            .as_deref(),
        Some("thumbnail")
    );
    assert_eq!(
        manifest.capabilities.directory_actions[0]
            .provides
            .as_deref(),
        Some("thumbnail")
    );
}

#[test]
fn singleton_resource_provider_label_is_wire_optional() {
    let mut value = manifest_document();
    value["capabilities"]["resource_actions"][0]["provides"] = json!("text_read");
    value["capabilities"]["resource_actions"][0]
        .as_object_mut()
        .unwrap()
        .remove("label");

    let manifest = canonical_manifest(&value).unwrap();
    assert!(manifest.capabilities.resource_actions[0].label.is_none());
    assert!(
        serde_json::to_value(manifest).unwrap()["capabilities"]["resource_actions"][0]
            .get("label")
            .is_none()
    );
}

#[test]
fn directory_request_and_output_have_separate_wire_effects() {
    let request: PluginDirectoryActionRequest = serde_json::from_value(json!({
        "action": "example.plugin.organize",
        "access": "write",
        "input": {},
        "directory": {
            "id": "01900000-0000-7000-8000-000000000001",
            "parent_id": "00000000-0000-0000-0000-000000000000",
            "path": "library",
            "name": "library",
            "kind": "example:collection",
            "revision": 3,
            "created_at": "2026-07-28T00:00:00Z",
            "updated_at": "2026-07-28T00:00:00Z"
        },
        "directory_ref": "directory:reference:call-scoped"
    }))
    .unwrap();
    assert_eq!(request.directory.path, "library");

    let output: PluginDirectoryActionOutput = serde_json::from_value(json!({
        "view": "json",
        "data": {"organized": true},
        "effects": [{"type": "create_child", "name": "covers", "kind": "core:directory"}]
    }))
    .unwrap();
    assert_eq!(output.effects.len(), 1);
}

#[test]
fn request_and_output_wire_shapes_match_the_current_goldens() {
    assert_golden_round_trip::<PluginManifest>(include_str!("fixtures/manifest-v2.json"));
    assert_golden_round_trip::<PluginResourceActionRequest>(include_str!(
        "fixtures/action-request-inline-v2.json"
    ));
    assert_golden_round_trip::<PluginResourceActionRequest>(include_str!(
        "fixtures/action-request-reference-v2.json"
    ));
    assert_golden_round_trip::<PluginResourceActionOutput>(include_str!(
        "fixtures/action-output-v2.json"
    ));
    assert_golden_round_trip::<PluginResourceActionOutput>(include_str!(
        "fixtures/action-output-download-v2.json"
    ));
    assert_golden_round_trip::<PluginActionFailure>(include_str!(
        "fixtures/action-failure-v2.json"
    ));
}

#[test]
fn context_specific_encodings_reject_invalid_wire_combinations() {
    let mut request: Value =
        serde_json::from_str(include_str!("fixtures/action-request-inline-v2.json")).unwrap();
    request["content"]["encoding"] = json!("handle");
    assert!(serde_json::from_value::<PluginResourceActionRequest>(request).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/action-output-v2.json")).unwrap();
    output["effects"][0]["encoding"] = json!("url");
    assert!(serde_json::from_value::<PluginResourceActionOutput>(output).is_err());

    let mut output: Value =
        serde_json::from_str(include_str!("fixtures/action-output-v2.json")).unwrap();
    output["effects"][0]["checksum"] = json!({
        "kind": "sha256",
        "value": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    });
    assert!(serde_json::from_value::<PluginResourceActionOutput>(output).is_err());
}
