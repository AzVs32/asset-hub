use super::*;

#[test]
fn normalizes_sub_limits_and_rejects_zero_values() {
    let policy = PluginExecutionPolicy::new(8, 9, 10, 16, 16, 1, 1, 1).unwrap();
    assert_eq!(policy.max_inline_content_bytes(), 8);
    assert_eq!(policy.max_content_read_bytes(), 8);
    assert!(PluginExecutionPolicy::new(8, 4, 4, 0, 16, 1, 1, 1).is_err());
}
