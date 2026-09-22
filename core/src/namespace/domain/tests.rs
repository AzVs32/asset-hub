use crate::namespace::error::NamespaceError;
use std::collections::{BTreeSet, HashSet};

use super::{DriverPath, EntryName, VirtualPath};

#[test]
fn driver_path_preserves_driver_specific_syntax() {
    let raw = r"bucket/prefix\\..//object";
    let path = DriverPath::new(raw);

    assert_eq!(path.as_str(), raw);
}

#[test]
fn driver_path_allows_an_empty_driver_root() {
    assert_eq!(DriverPath::new("").as_str(), "");
}

#[test]
fn virtual_path_try_from_preserves_a_canonical_path() {
    let path = VirtualPath::try_from("/assets/images/cover.png").unwrap();

    assert_eq!(path.as_str(), "/assets/images/cover.png");
}

#[test]
fn virtual_path_try_from_preserves_root() {
    assert_eq!(VirtualPath::try_from("/").unwrap().as_str(), "/");
}

#[test]
fn virtual_path_try_from_rejects_relative_paths() {
    assert_eq!(
        VirtualPath::try_from("assets/images").unwrap_err(),
        NamespaceError::VirtualPathNotAbsolute
    );
}

#[test]
fn virtual_path_try_from_rejects_backslashes() {
    assert_eq!(
        VirtualPath::try_from(r"\assets\images").unwrap_err(),
        NamespaceError::VirtualPathNotAbsolute
    );
    assert_eq!(
        VirtualPath::try_from(r"/assets\images").unwrap_err(),
        NamespaceError::EntryNameContainsBackslash
    );
}

#[test]
fn virtual_path_try_from_rejects_repeated_and_trailing_separators() {
    assert_eq!(
        VirtualPath::try_from("/assets//images").unwrap_err(),
        NamespaceError::VirtualPathContainsRepeatedSeparator
    );
    assert_eq!(
        VirtualPath::try_from("/assets/").unwrap_err(),
        NamespaceError::VirtualPathHasTrailingSeparator
    );
}

#[test]
fn virtual_path_try_from_rejects_dot_segments() {
    assert_eq!(
        VirtualPath::try_from("/assets/./images").unwrap_err(),
        NamespaceError::EntryNameIsDot
    );
}

#[test]
fn virtual_path_try_from_rejects_dot_dot_segments() {
    assert_eq!(
        VirtualPath::try_from("/assets/../images").unwrap_err(),
        NamespaceError::EntryNameIsDotDot
    );
}

#[test]
fn virtual_path_try_from_rejects_nul_and_control_characters() {
    assert_eq!(
        VirtualPath::try_from("/assets/\0images").unwrap_err(),
        NamespaceError::EntryNameContainsControlCharacter
    );
    assert_eq!(
        VirtualPath::try_from("/assets/new\nline").unwrap_err(),
        NamespaceError::EntryNameContainsControlCharacter
    );
}

#[test]
fn virtual_path_try_from_preserves_spaces_and_is_case_sensitive() {
    let spaced = VirtualPath::try_from("/Asset Library/My File.mp4").unwrap();
    let lower = VirtualPath::try_from("/asset library/my file.mp4").unwrap();

    assert_eq!(spaced.as_str(), "/Asset Library/My File.mp4");
    assert_ne!(spaced, lower);
}

#[test]
fn virtual_path_try_from_preserves_unicode_without_normalization() {
    let composed = VirtualPath::try_from("/caf\u{e9}").unwrap();
    let decomposed = VirtualPath::try_from("/cafe\u{301}").unwrap();

    assert_ne!(composed, decomposed);
    assert_eq!(composed.as_str(), "/caf\u{e9}");
    assert_eq!(decomposed.as_str(), "/cafe\u{301}");
}

#[test]
fn virtual_path_enforces_path_and_segment_byte_limits() {
    const MAX_PATH_BYTES: usize = 4096;
    const MAX_SEGMENT_BYTES: usize = 255;

    let oversized_segment = "a".repeat(MAX_SEGMENT_BYTES + 1);
    assert_eq!(
        VirtualPath::try_from(format!("/{oversized_segment}").as_str()).unwrap_err(),
        NamespaceError::EntryNameTooLong {
            length: MAX_SEGMENT_BYTES + 1,
            max: MAX_SEGMENT_BYTES,
        }
    );

    let segment = "a".repeat(MAX_SEGMENT_BYTES);
    let maximum_path = format!("/{}", vec![segment.clone(); 16].join("/"));
    assert_eq!(maximum_path.len(), MAX_PATH_BYTES);
    let maximum_path = VirtualPath::try_from(maximum_path.as_str()).unwrap();
    assert_eq!(
        maximum_path.join_segment("b").unwrap_err(),
        NamespaceError::VirtualPathTooLong {
            length: MAX_PATH_BYTES + 2,
            max: MAX_PATH_BYTES,
        }
    );

    let oversizedriver_path = format!("/{}", vec![segment; 17].join("/"));
    assert_eq!(
        VirtualPath::try_from(oversizedriver_path.as_str()).unwrap_err(),
        NamespaceError::VirtualPathTooLong {
            length: oversizedriver_path.len(),
            max: MAX_PATH_BYTES,
        }
    );
}

#[test]
fn virtual_path_supports_standard_string_interfaces_and_ordered_keys() {
    let path = VirtualPath::try_from("/assets/file.txt").unwrap();

    assert_eq!(path.to_string(), "/assets/file.txt");
    assert_eq!(path.as_ref(), "/assets/file.txt");
    assert!(HashSet::from([path.clone()]).contains(&path));
    assert!(BTreeSet::from([path.clone()]).contains(&path));
}

#[test]
fn virtual_path_joins_one_validated_segment() {
    const MAX_SEGMENT_BYTES: usize = 255;

    let root = VirtualPath::root();
    let assets = root.join_segment("Asset Library").unwrap();
    let file = assets.join_segment("My File.mp4").unwrap();

    assert_eq!(assets.as_str(), "/Asset Library");
    assert_eq!(file.as_str(), "/Asset Library/My File.mp4");
    assert_eq!(
        assets.join_segment("").unwrap_err(),
        NamespaceError::EntryNameEmpty
    );
    assert_eq!(
        assets.join_segment("a/b").unwrap_err(),
        NamespaceError::EntryNameContainsSeparator
    );
    assert_eq!(
        assets.join_segment(".").unwrap_err(),
        NamespaceError::EntryNameIsDot
    );
    assert_eq!(
        assets.join_segment("..").unwrap_err(),
        NamespaceError::EntryNameIsDotDot
    );
    assert_eq!(
        assets.join_segment(r"a\b").unwrap_err(),
        NamespaceError::EntryNameContainsBackslash
    );
    assert_eq!(
        assets.join_segment("new\nline").unwrap_err(),
        NamespaceError::EntryNameContainsControlCharacter
    );
    let oversized_segment = "a".repeat(MAX_SEGMENT_BYTES + 1);
    assert_eq!(
        assets.join_segment(&oversized_segment).unwrap_err(),
        NamespaceError::EntryNameTooLong {
            length: MAX_SEGMENT_BYTES + 1,
            max: MAX_SEGMENT_BYTES,
        }
    );
}

#[test]
fn entry_name_is_a_validated_virtual_path_segment() {
    let name = EntryName::try_from("My File.mp4").unwrap();

    assert_eq!(name.as_str(), "My File.mp4");
    assert_eq!(name.as_ref(), "My File.mp4");
    assert_eq!(name.to_string(), "My File.mp4");
    assert_eq!(
        EntryName::try_from("nested/file").unwrap_err(),
        NamespaceError::EntryNameContainsSeparator
    );
    assert_eq!(
        EntryName::try_from("..").unwrap_err(),
        NamespaceError::EntryNameIsDotDot
    );
}

#[test]
fn virtual_path_joins_an_already_validated_entry_name() {
    let name = EntryName::try_from("cover.jpg").unwrap();
    let path = VirtualPath::try_from("/assets")
        .unwrap()
        .join_name(&name)
        .unwrap();

    assert_eq!(path.as_str(), "/assets/cover.jpg");
}

#[test]
fn virtual_path_root_has_no_parent_or_name() {
    let root = VirtualPath::root();

    assert!(root.is_root());
    assert_eq!(root.depth(), 0);
    assert_eq!(root.parent(), None);
    assert_eq!(root.name(), None);
}

#[test]
fn virtual_path_exposes_its_structure() {
    let path = VirtualPath::try_from("/a/b/c").unwrap();

    assert!(!path.is_root());
    assert_eq!(path.depth(), 3);
    assert_eq!(path.parent().unwrap().as_str(), "/a/b");
    assert_eq!(path.name(), Some("c"));
}

#[test]
fn virtual_path_ancestor_checks_use_complete_segments() {
    let root = VirtualPath::root();
    let a = VirtualPath::try_from("/a").unwrap();
    let nested = VirtualPath::try_from("/a/b").unwrap();
    let similar = VirtualPath::try_from("/abc").unwrap();

    assert!(root.is_ancestor_of(&a));
    assert!(a.is_ancestor_of(&nested));
    assert!(a.is_ancestor_or_self_of(&a));
    assert!(!a.is_ancestor_of(&a));
    assert!(!a.is_ancestor_of(&similar));
    assert!(!a.is_ancestor_or_self_of(&similar));
}

#[test]
fn virtual_path_strips_ancestor_prefixes_into_relative_paths() {
    let root = VirtualPath::root();
    let movies = VirtualPath::try_from("/movies").unwrap();
    let file = VirtualPath::try_from("/movies/2026/a.mp4").unwrap();
    let similar = VirtualPath::try_from("/movie").unwrap();

    let from_mount = file.strip_prefix(&movies).unwrap();
    assert_eq!(from_mount.as_str(), "2026/a.mp4");
    assert_eq!(from_mount.depth(), 2);
    assert_eq!(from_mount.name(), Some("a.mp4"));
    assert_eq!(from_mount.to_string(), "2026/a.mp4");
    assert_eq!(from_mount.as_ref(), "2026/a.mp4");

    let from_root = file.strip_prefix(&root).unwrap();
    assert_eq!(from_root.as_str(), "movies/2026/a.mp4");

    let mount_root = movies.strip_prefix(&movies).unwrap();
    assert!(mount_root.is_empty());
    assert_eq!(mount_root.depth(), 0);
    assert_eq!(mount_root.name(), None);

    assert_eq!(file.strip_prefix(&similar), None);
}
