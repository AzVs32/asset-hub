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
