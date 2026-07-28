//! Content ABI 范围值对象的边界测试。

use super::*;

#[test]
fn bounds_ranges_without_overflow() {
    assert_eq!(
        PluginContentRange::new(4, 9)
            .unwrap()
            .bounded(10, 3)
            .unwrap(),
        PluginContentRange {
            offset: 4,
            length: 3
        }
    );
    assert!(PluginContentRange::new(u64::MAX, 1).is_err());
    assert!(
        PluginContentRange::new(11, 0)
            .unwrap()
            .bounded(10, 3)
            .is_err()
    );
    assert!(
        serde_json::from_value::<PluginContentRange>(serde_json::json!({
            "offset": u64::MAX,
            "length": 1
        }))
        .is_err()
    );
}
