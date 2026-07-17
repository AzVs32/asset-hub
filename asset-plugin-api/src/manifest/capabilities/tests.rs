use super::*;
use serde_json::json;

#[test]
fn action_requirements_reject_removed_snapshot_flags() {
    let result = serde_json::from_value::<ActionRequirements>(json!({
        "resource": true,
        "metadata": true,
        "content": true
    }));

    assert!(result.is_err());
}
