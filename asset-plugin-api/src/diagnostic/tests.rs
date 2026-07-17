use super::*;

#[test]
fn action_failure_has_a_stable_serialized_shape() {
    let failure = PluginActionFailure::new(PluginDiagnostic::error(
        codes::INVALID_INPUT,
        "missing field",
    ));
    let value = serde_json::to_value(failure).unwrap();
    assert_eq!(value["error"]["code"], codes::INVALID_INPUT);
    assert_eq!(value["error"]["severity"], "error");
    assert_eq!(value["error"]["retryable"], false);
}
