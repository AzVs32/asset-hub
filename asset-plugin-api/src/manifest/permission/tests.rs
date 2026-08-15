//! Manifest 权限作用域的反序列化和查询行为测试。

use super::*;
use serde_json::json;

#[test]
fn removed_generic_resource_write_permissions_are_rejected() {
    for permission in ["resource.write", "resource.derived_asset.write"] {
        let result = serde_json::from_value::<PluginPermissions>(json!({
            "allow": [permission]
        }));

        assert!(result.is_err(), "`{permission}` must not be accepted");
    }
}
