use super::*;

#[test]
fn directory_is_built_from_parent_and_single_name() {
    let parent = ResourceDirectory::from_path("projects").unwrap();
    let directory = parent.child(" images ").unwrap();
    assert_eq!(directory.path(), "projects/images");
    assert_eq!(directory.parent_path(), "projects");
    assert_eq!(directory.name(), "images");
    assert!(parent.child("../secret").is_err());
}

#[test]
fn path_constructor_supports_root_and_normalizes_segments() {
    assert!(ResourceDirectory::from_path("  ").unwrap().is_root());
    assert_eq!(
        ResourceDirectory::from_path(" projects\\images/./raw ")
            .unwrap()
            .path(),
        "projects/images/raw"
    );
}

#[test]
fn serde_uses_the_path_representation() {
    let directory = ResourceDirectory::from_path("projects/images").unwrap();
    let json = serde_json::to_string(&directory).unwrap();
    assert_eq!(json, "\"projects/images\"");
    assert_eq!(
        serde_json::from_str::<ResourceDirectory>(&json).unwrap(),
        directory
    );
}

#[test]
fn contains_obeys_directory_segment_boundaries() {
    let root = ResourceDirectory::root();
    let home = ResourceDirectory::from_path("users/alice").unwrap();
    let child = ResourceDirectory::from_path("users/alice/photos").unwrap();
    let sibling = ResourceDirectory::from_path("users/alice2").unwrap();

    assert!(root.contains(&home));
    assert!(home.contains(&home));
    assert!(home.contains(&child));
    assert!(!home.contains(&sibling));
}

#[test]
fn rehydrate_rejects_noncanonical_or_inconsistent_fields() {
    assert!(
        ResourceDirectory::rehydrate(
            " projects/images ".to_owned(),
            "projects".to_owned(),
            "images".to_owned(),
        )
        .is_err()
    );
    assert!(
        ResourceDirectory::rehydrate(
            "projects/images".to_owned(),
            "other".to_owned(),
            "images".to_owned(),
        )
        .is_err()
    );
}
