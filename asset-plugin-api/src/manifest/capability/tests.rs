//! Manifest capability 的 JSON 形状与领域转换测试。

use super::*;
use serde_json::json;

#[test]
fn action_requirements_reject_removed_resource_flag() {
    let result = serde_json::from_value::<ActionRequirements>(json!({
        "resource": true,
        "content": true
    }));

    assert!(result.is_err());
}
