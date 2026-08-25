//! 完整 Manifest 文档的跨字段校验测试。

use super::*;
use crate::protocol::PLUGIN_API_VERSION;
use std::collections::BTreeMap;

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
                "output": {"views": ["json"]}
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
        serde_json::from_value::<PluginManifestDocument>(document.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    document["manifest_version"] = serde_json::json!(MANIFEST_VERSION);
    for unsupported in ["asset-hub.plugin-api@1", "asset-hub.plugin-api@4"] {
        document["runtime"]["plugin_api"] = serde_json::json!(unsupported);
        assert!(
            serde_json::from_value::<PluginManifestDocument>(document.clone())
                .unwrap()
                .validate()
                .is_err()
        );
    }
    document["runtime"]
        .as_object_mut()
        .unwrap()
        .remove("plugin_api");
    assert!(serde_json::from_value::<PluginManifestDocument>(document).is_err());
}

#[test]
fn manifest_accepts_multi_segment_kind_ids_and_directory_parent_constraints() {
    let mut document = manifest_document();
    document["capabilities"]["directory_kinds"] = serde_json::json!([
        {
            "kind": "example:directory:collection",
            "parent": "core:directory",
            "default_child_kind": "example:directory:collection:item",
            "label": "Collection"
        },
        {
            "kind": "example:directory:collection:item",
            "parent": "example:directory:collection",
            "allowed_parent_kinds": ["example:directory:collection"],
            "label": "Collection item"
        }
    ]);

    let manifest = serde_json::from_value::<PluginManifestDocument>(document)
        .unwrap()
        .validate()
        .unwrap();
    assert_eq!(
        manifest.capabilities.directory_kinds[1].allowed_parent_kinds,
        ["example:directory:collection"]
    );
    assert_eq!(
        manifest.capabilities.directory_kinds[0]
            .default_child_kind
            .as_deref(),
        Some("example:directory:collection:item")
    );
}

#[test]
fn directory_content_access_requires_resource_content_permissions() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"] = serde_json::json!([]);
    document["capabilities"]["directory_actions"] = serde_json::json!([{
        "id": "example.plugin.inspect",
        "label": "Inspect",
        "handler": "inspect",
        "requires": {"resources": "content"},
        "output": {"views": ["json"]}
    }]);
    document["permissions"]["allow"] =
        serde_json::json!(["directory.read", "directory.resources.list"]);

    assert!(
        serde_json::from_value::<PluginManifestDocument>(document.clone())
            .unwrap()
            .validate()
            .unwrap_err()
            .message
            .contains("resource.read and resource.content.read")
    );

    document["permissions"]["allow"] = serde_json::json!([
        "directory.read",
        "directory.resources.list",
        "resource.read",
        "resource.content.read"
    ]);
    serde_json::from_value::<PluginManifestDocument>(document)
        .unwrap()
        .validate()
        .unwrap();
}

#[test]
fn create_tree_requires_both_directory_and_resource_creation_permissions() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"] = serde_json::json!([]);
    document["capabilities"]["directory_actions"] = serde_json::json!([{
        "id": "example.plugin.scaffold",
        "label": "Scaffold",
        "handler": "scaffold",
        "access": "write",
        "output": {"effects": ["create_tree"]}
    }]);
    document["permissions"]["allow"] =
        serde_json::json!(["directory.read", "directory.create_child"]);

    assert!(
        serde_json::from_value::<PluginManifestDocument>(document.clone())
            .unwrap()
            .validate()
            .unwrap_err()
            .message
            .contains("resource.create")
    );

    document["permissions"]["allow"] = serde_json::json!([
        "directory.read",
        "directory.create_child",
        "resource.create"
    ]);
    serde_json::from_value::<PluginManifestDocument>(document)
        .unwrap()
        .validate()
        .unwrap();
}

#[test]
fn manifest_rejects_non_canonical_plugin_owner_ids() {
    for id in ["Example.Plugin", "example..plugin", ".example"] {
        let mut document = manifest_document();
        document["plugin"]["id"] = serde_json::json!(id);
        let manifest = serde_json::from_value::<PluginManifestDocument>(document).unwrap();
        assert!(manifest.validate().is_err(), "`{id}` must be rejected");
    }
}

#[test]
fn action_ids_are_owned_by_the_declaring_plugin() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]["id"] =
        serde_json::json!("another.plugin.action");
    let error = serde_json::from_value::<PluginManifestDocument>(document)
        .unwrap()
        .validate()
        .unwrap_err();
    assert_eq!(error.code, ManifestValidationCode::ActionOwnerMismatch);
    assert_eq!(error.path, "$.capabilities.resource_actions[0].id");
}

#[test]
fn content_delivery_requires_content() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]["requires"] =
        serde_json::json!({"content": false, "content_delivery": "inline"});
    let error = serde_json::from_value::<PluginManifestDocument>(document)
        .unwrap()
        .validate()
        .unwrap_err();
    assert_eq!(error.code, ManifestValidationCode::InconsistentDeclaration);
}

#[test]
fn manifest_rejects_host_owned_builtin_runtime() {
    let mut document = manifest_document();
    document["runtime"] = serde_json::json!({"type": "builtin"});

    assert!(serde_json::from_value::<PluginManifestDocument>(document).is_err());
}

#[test]
fn lock_uses_one_flat_integrity_map() {
    let manifest = serde_json::from_value::<PluginManifestDocument>(manifest_document())
        .unwrap()
        .validate()
        .unwrap();
    let digest = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let lock = PluginManifestLock {
        manifest_version: MANIFEST_VERSION,
        plugin_id: manifest.plugin_id().to_string(),
        integrity: BTreeMap::from([
            (
                PluginPackagePath::new(PLUGIN_WASM_FILE_NAME).unwrap(),
                digest.to_string(),
            ),
            (
                PluginPackagePath::new(PLUGIN_WEB_ENTRY_FILE_NAME).unwrap(),
                digest.to_string(),
            ),
            (
                PluginPackagePath::new("assets/app.js").unwrap(),
                digest.to_string(),
            ),
        ]),
    };

    lock.validate_for(&manifest).unwrap();
    let document = serde_json::to_value(lock).unwrap();
    assert!(document.get("integrity").is_some());
    assert!(document.get("runtime").is_none());
    assert!(document.get("web").is_none());
}

#[test]
fn lock_rejects_non_flat_integrity_groups() {
    let grouped_lock = serde_json::json!({
        "manifest_version": MANIFEST_VERSION,
        "plugin_id": "example.plugin",
        "integrity": {},
        "runtime": {"wasm_sha256": "unused"},
        "web": {"integrity": {}}
    });

    assert!(serde_json::from_value::<PluginManifestLock>(grouped_lock).is_err());
}

#[test]
fn package_paths_are_platform_independent_and_canonical() {
    assert_eq!(
        PluginPackagePath::new("web/assets/app.js")
            .unwrap()
            .as_str(),
        "web/assets/app.js"
    );
    for invalid in [
        "/plugin.wasm",
        "../plugin.wasm",
        "C:/plugin.wasm",
        "web\\app.js",
        "web//app.js",
    ] {
        assert!(PluginPackagePath::new(invalid).is_err(), "`{invalid}`");
    }
}

#[test]
fn manifest_rejects_unknown_fields_at_every_level() {
    let mut document = manifest_document();
    document["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PluginManifestDocument>(document).is_err());

    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]["applies_to"]["typo"] = serde_json::json!([]);
    assert!(serde_json::from_value::<PluginManifestDocument>(document).is_err());

    let mut document = manifest_document();
    document["runtime"]["wais"] = serde_json::json!(false);
    assert!(serde_json::from_value::<PluginManifestDocument>(document).is_err());

    let mut document = manifest_document();
    document["runtime"]["wasm"] = serde_json::json!("custom.wasm");
    assert!(serde_json::from_value::<PluginManifestDocument>(document).is_err());

    let mut document = manifest_document();
    document["web"] = serde_json::json!({"root": "dist"});
    assert!(serde_json::from_value::<PluginManifestDocument>(document).is_err());
}

#[test]
fn manifest_validates_provided_capability_ids() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]["provides"] =
        serde_json::json!("Resource.Thumbnail");

    assert!(
        serde_json::from_value::<PluginManifestDocument>(document)
            .unwrap()
            .validate()
            .is_err()
    );

    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]["provides"] = serde_json::json!("custom_view");
    assert!(
        serde_json::from_value::<PluginManifestDocument>(document)
            .unwrap()
            .validate()
            .unwrap_err()
            .message
            .contains("unsupported capability")
    );
}

#[test]
fn resource_capability_provider_may_omit_an_inherited_label() {
    let mut document = manifest_document();
    let action = &mut document["capabilities"]["resource_actions"][0];
    action.as_object_mut().unwrap().remove("label");
    action["provides"] = serde_json::json!("view");
    action["output"] = serde_json::json!({"views": ["plugin_frame"]});

    let manifest = serde_json::from_value::<PluginManifestDocument>(document)
        .unwrap()
        .validate()
        .unwrap();
    assert!(manifest.capabilities.resource_actions[0].label.is_none());
}

#[test]
fn resource_action_without_a_capability_still_requires_a_label() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"][0]
        .as_object_mut()
        .unwrap()
        .remove("label");

    assert!(
        serde_json::from_value::<PluginManifestDocument>(document)
            .unwrap()
            .validate()
            .unwrap_err()
            .message
            .contains("label is required")
    );
}

#[test]
fn edit_provider_requires_content_replace_permission() {
    let mut document = manifest_document();
    let action = &mut document["capabilities"]["resource_actions"][0];
    action["provides"] = serde_json::json!("edit");
    action["access"] = serde_json::json!("write");
    action["output"] = serde_json::json!({"views": ["plugin_frame"]});

    let error = serde_json::from_value::<PluginManifestDocument>(document.clone())
        .unwrap()
        .validate()
        .unwrap_err();
    assert!(error.message.contains("resource.content.replace"));

    document["permissions"]["allow"] =
        serde_json::json!(["resource.read", "resource.content.replace"]);
    serde_json::from_value::<PluginManifestDocument>(document)
        .unwrap()
        .validate()
        .unwrap();
}

#[test]
fn delete_effect_requires_write_access_and_the_matching_permission() {
    let mut resource_document = manifest_document();
    let action = &mut resource_document["capabilities"]["resource_actions"][0];
    action["access"] = serde_json::json!("write");
    action["output"] = serde_json::json!({"effects": ["delete"]});
    resource_document["permissions"]["allow"] =
        serde_json::json!(["resource.read", "resource.delete"]);
    serde_json::from_value::<PluginManifestDocument>(resource_document.clone())
        .unwrap()
        .validate()
        .unwrap();

    resource_document["permissions"]["allow"] = serde_json::json!(["resource.read"]);
    assert!(
        serde_json::from_value::<PluginManifestDocument>(resource_document)
            .unwrap()
            .validate()
            .unwrap_err()
            .message
            .contains("resource.delete")
    );

    let mut directory_document = manifest_document();
    directory_document["capabilities"]["resource_actions"] = serde_json::json!([]);
    directory_document["capabilities"]["directory_actions"] = serde_json::json!([{
        "id": "example.plugin.delete_directory",
        "label": "Delete directory",
        "handler": "delete_directory",
        "access": "write",
        "output": {"effects": ["delete"]}
    }]);
    directory_document["permissions"]["allow"] =
        serde_json::json!(["directory.read", "directory.delete"]);
    serde_json::from_value::<PluginManifestDocument>(directory_document)
        .unwrap()
        .validate()
        .unwrap();
}

#[test]
fn directory_workspace_provider_has_an_exclusive_read_only_frame_contract() {
    let mut document = manifest_document();
    document["capabilities"]["resource_actions"] = serde_json::json!([]);
    document["capabilities"]["directory_actions"] = serde_json::json!([{
        "id": "example.plugin.workspace",
        "provides": "workspace",
        "label": "Custom workspace",
        "handler": "render_workspace",
        "applies_to": {"kinds": ["example:collection"]},
        "access": "read",
        "requires": {"children": true, "resources": "metadata"},
        "output": {"views": ["plugin_frame", "json"]},
        "ui": {"locations": ["directory_workspace"]}
    }]);
    document["permissions"]["allow"] = serde_json::json!([
        "directory.read",
        "directory.children.list",
        "directory.resources.list"
    ]);

    serde_json::from_value::<PluginManifestDocument>(document.clone())
        .unwrap()
        .validate()
        .unwrap();

    let mut mixed_locations = document.clone();
    mixed_locations["capabilities"]["directory_actions"][0]["ui"]["locations"] =
        serde_json::json!(["directory_workspace", "directory_context_menu"]);
    assert!(
        serde_json::from_value::<PluginManifestDocument>(mixed_locations)
            .unwrap()
            .validate()
            .unwrap_err()
            .message
            .contains("use only directory_workspace")
    );

    document["capabilities"]["directory_actions"][0]["access"] = serde_json::json!("write");
    document["permissions"]["allow"] = serde_json::json!([
        "directory.read",
        "directory.write",
        "directory.children.list",
        "directory.resources.list"
    ]);
    assert!(
        serde_json::from_value::<PluginManifestDocument>(document)
            .unwrap()
            .validate()
            .unwrap_err()
            .message
            .contains("workspace provider must be read-only")
    );
}
