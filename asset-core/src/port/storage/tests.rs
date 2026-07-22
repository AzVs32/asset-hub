use super::*;
use crate::domain::StorageKey;

#[test]
fn prefix_is_canonical_and_uses_segment_boundaries() {
    let prefix = StoragePrefix::new("assets/images/").unwrap();
    assert_eq!(prefix.as_str(), "assets/images");
    assert!(prefix.contains(&StorageKey::new("assets/images/a.png").unwrap()));
    assert!(!prefix.contains(&StorageKey::new("assets/images2/a.png").unwrap()));
    assert!(StoragePrefix::new("a//b").is_err());
}

#[test]
fn prefix_preserves_spaces_exactly() {
    let prefix = StoragePrefix::new(" library / project A /").unwrap();

    assert_eq!(prefix.as_str(), " library / project A ");
    assert!(prefix.contains(&StorageKey::new(" library / project A / design 01.md").unwrap()));
    assert!(!prefix.contains(&StorageKey::new("library/project A/design 01.md").unwrap()));
}
