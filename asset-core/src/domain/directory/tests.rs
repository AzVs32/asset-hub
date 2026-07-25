use super::*;
use crate::error::DirectoryError;
use serde_json::json;

#[test]
fn directory_kind_is_limited_to_256_characters() {
    let max_length = format!("a:{}", "b".repeat(254));
    let too_long = format!("a:{}", "b".repeat(255));

    assert!(DirectoryKind::try_new(max_length).is_ok());
    assert!(matches!(
        DirectoryKind::try_new(too_long),
        Err(DirectoryError::TooLong {
            field: "directory.kind",
            max: 256,
        })
    ));
}

#[test]
fn directory_path_is_built_from_parent_and_single_name() {
    let parent = DirectoryPath::from_path("projects").unwrap();
    let directory = parent.child(" images ").unwrap();

    assert_eq!(directory.path(), "projects/ images ");
    assert_eq!(directory.parent_path(), "projects");
    assert_eq!(directory.name(), " images ");
    assert!(parent.child("../secret").is_err());
}

#[test]
fn directory_path_supports_root_and_normalizes_segments() {
    assert!(DirectoryPath::from_path("").unwrap().is_root());
    assert!(DirectoryPath::from_path("  ").is_err());
    assert_eq!(
        DirectoryPath::from_path("projects\\images/./raw")
            .unwrap()
            .path(),
        "projects/images/raw"
    );
}

#[test]
fn directory_path_rejects_internal_storage_namespace() {
    for path in [".asset-hub", ".asset-hub/trash"] {
        assert!(matches!(
            DirectoryPath::from_path(path),
            Err(DirectoryError::InvalidFormat {
                field: "directory.path",
                ..
            })
        ));
    }
}

#[test]
fn directory_path_serde_uses_the_path_representation() {
    let directory = DirectoryPath::from_path("projects/images").unwrap();
    let json = serde_json::to_string(&directory).unwrap();

    assert_eq!(json, "\"projects/images\"");
    assert_eq!(
        serde_json::from_str::<DirectoryPath>(&json).unwrap(),
        directory
    );
}

#[test]
fn directory_path_contains_obeys_segment_boundaries() {
    let root = DirectoryPath::root();
    let home = DirectoryPath::from_path("users/alice").unwrap();
    let child = DirectoryPath::from_path("users/alice/photos").unwrap();
    let sibling = DirectoryPath::from_path("users/alice2").unwrap();

    assert!(root.contains(&home));
    assert!(home.contains(&home));
    assert!(home.contains(&child));
    assert!(!home.contains(&sibling));
}

#[test]
fn directory_is_an_independent_aggregate_with_a_stable_identity() {
    let parent = DirectoryId::new();
    let mut directory = Directory::new(parent, "Games").unwrap();
    let id = directory.id();

    directory.rename("Library").unwrap();
    directory.move_to(DirectoryId::new()).unwrap();
    directory.change_kind("azvs.game:library").unwrap();
    directory
        .replace_metadata(json!({"platform": "windows"}))
        .unwrap();

    assert_eq!(directory.id(), id);
    assert_eq!(directory.name(), "Library");
    assert_eq!(directory.kind().as_str(), "azvs.game:library");
    assert_eq!(directory.metadata(), &json!({"platform": "windows"}));
}

#[test]
fn root_directory_has_fixed_identity_and_cannot_be_mutated_as_a_child() {
    let mut root = Directory::root();

    assert_eq!(root.id(), DirectoryId::root());
    assert!(root.parent_id().is_none());
    assert!(root.rename("root").is_err());
    assert!(root.move_to(DirectoryId::new()).is_err());
}

#[test]
fn directory_rejects_self_parent_and_non_object_metadata() {
    let mut directory = Directory::new(DirectoryId::root(), "Games").unwrap();

    assert!(directory.move_to(directory.id()).is_err());
    assert!(directory.replace_metadata(json!(["invalid"])).is_err());
}
