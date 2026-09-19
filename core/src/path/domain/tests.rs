use crate::path::error::DomainError;

use super::{DPath, VPath};

#[test]
fn d_path_preserves_driver_specific_syntax() {
    let raw = r"bucket/prefix\\..//object";
    let path = DPath::new(raw);

    assert_eq!(path.as_str(), raw);
}

#[test]
fn d_path_allows_an_empty_driver_root() {
    assert_eq!(DPath::new("").as_str(), "");
}

#[test]
fn v_path_parse_normalizes_separators_and_segments() {
    let path = VPath::parse(r"\assets\\images/./cover.png//").unwrap();

    assert_eq!(path.as_str(), "/assets/images/cover.png");
}

#[test]
fn v_path_parse_preserves_root() {
    assert_eq!(VPath::parse(r"\\./").unwrap().as_str(), "/");
}

#[test]
fn v_path_parse_rejects_relative_paths() {
    assert_eq!(
        VPath::parse("assets/images").unwrap_err(),
        DomainError::VPathNotAbsolute
    );
}

#[test]
fn v_path_parse_rejects_dot_dot_segments() {
    assert_eq!(
        VPath::parse("/assets/../images").unwrap_err(),
        DomainError::VPathContainsDotDotSegment
    );
}

#[test]
fn v_path_root_has_no_parent_or_name() {
    let root = VPath::root();

    assert!(root.is_root());
    assert_eq!(root.depth(), 0);
    assert_eq!(root.parent(), None);
    assert_eq!(root.name(), None);
}

#[test]
fn v_path_exposes_its_structure() {
    let path = VPath::parse("/a/b/c").unwrap();

    assert!(!path.is_root());
    assert_eq!(path.depth(), 3);
    assert_eq!(path.parent().unwrap().as_str(), "/a/b");
    assert_eq!(path.name(), Some("c"));
}

#[test]
fn v_path_ancestor_checks_use_complete_segments() {
    let root = VPath::root();
    let a = VPath::parse("/a").unwrap();
    let nested = VPath::parse("/a/b").unwrap();
    let similar = VPath::parse("/abc").unwrap();

    assert!(root.is_ancestor_of(&a));
    assert!(a.is_ancestor_of(&nested));
    assert!(a.is_ancestor_or_self_of(&a));
    assert!(!a.is_ancestor_of(&a));
    assert!(!a.is_ancestor_of(&similar));
    assert!(!a.is_ancestor_or_self_of(&similar));
}
