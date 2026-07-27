use super::*;
use serde_json::json;

#[test]
fn accepts_only_fine_grained_permissions() {
    let permissions: PluginPermissions = serde_json::from_value(json!({
        "allow": ["resource.read", "content.read", "content.replace"],
        "network": false,
        "filesystem": false
    }))
    .unwrap();
    assert!(permissions.resource_read());
    assert!(permissions.content_read());
    assert!(permissions.content_replace());
    assert_eq!(
        serde_json::to_value(permissions).unwrap()["allow"],
        json!(["resource.read", "content.read", "content.replace"])
    );

    assert!(
        serde_json::from_value::<PluginPermissions>(json!({
            "resource": {"read": true, "write": false},
            "content": {"read": true, "write": true}
        }))
        .is_err()
    );
}

#[test]
fn directory_permissions_are_independent_capabilities() {
    let permissions: PluginPermissions = serde_json::from_value(json!({
        "allow": [
            "directory.read",
            "directory.children.list",
            "directory.resources.list",
            "directory.write",
            "directory.create_child"
        ]
    }))
    .unwrap();

    assert!(permissions.directory_read());
    assert!(permissions.directory_children_list());
    assert!(permissions.directory_resources_list());
    assert!(permissions.directory_write());
    assert!(permissions.directory_create_child());
    assert!(!permissions.resource_read());
}
