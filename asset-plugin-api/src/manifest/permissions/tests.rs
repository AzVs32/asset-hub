use super::*;
use serde_json::json;

#[test]
fn accepts_v2_and_serializes_canonical_v3_permissions() {
    let permissions: PluginPermissions = serde_json::from_value(json!({
        "resource": {"read": true, "write": false},
        "content": {"read": true, "write": true},
        "network": false,
        "filesystem": false
    }))
    .unwrap();
    assert!(permissions.resource_metadata_read());
    assert!(permissions.content_read());
    assert!(permissions.content_replace());
    let value = serde_json::to_value(permissions).unwrap();
    assert_eq!(
        value["allow"],
        json!(["resource.metadata.read", "content.read", "content.replace"])
    );
    assert!(value.get("resource").is_none());
}
