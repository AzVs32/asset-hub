//! Manifest 权限作用域的反序列化和查询行为测试。

use super::*;
use serde_json::json;

#[test]
fn fine_grained_permissions_round_trip() {
    let permissions: PluginPermissions = serde_json::from_value(json!({
        "allow": ["resource.read", "resource.content.read", "resource.content.replace"],
        "network": false,
        "filesystem": false
    }))
    .unwrap();
    assert!(permissions.resource_read());
    assert!(permissions.resource_content_read());
    assert!(permissions.resource_content_replace());
    assert_eq!(
        serde_json::to_value(permissions).unwrap()["allow"],
        json!([
            "resource.read",
            "resource.content.read",
            "resource.content.replace"
        ])
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
