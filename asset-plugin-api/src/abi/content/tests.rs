//! Plugin API 内容范围值对象的边界测试。

use super::*;

#[test]
fn bounds_ranges_without_overflow() {
    assert_eq!(
        ContentRange::new(4, 9)
            .unwrap()
            .constrain_to(10, 3)
            .unwrap(),
        ContentRange {
            offset: 4,
            length: 3
        }
    );
    assert!(ContentRange::new(u64::MAX, 1).is_err());
    assert!(
        ContentRange::new(11, 0)
            .unwrap()
            .constrain_to(10, 3)
            .is_err()
    );
    assert!(
        serde_json::from_value::<ContentRange>(serde_json::json!({
            "offset": u64::MAX,
            "length": 1
        }))
        .is_err()
    );
}
