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
