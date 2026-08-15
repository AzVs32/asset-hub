use super::*;
use crate::error::DirectoryError;

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
fn root_directory_has_fixed_identity_and_cannot_be_mutated_as_a_child() {
    let mut root = Directory::root();

    assert_eq!(root.id(), DirectoryId::root());
    assert!(root.parent_id().is_none());
    assert!(root.rename("root").is_err());
    assert!(root.move_to(DirectoryId::new()).is_err());
}

#[test]
fn directory_rejects_self_parent() {
    let mut directory = Directory::new(DirectoryId::root(), "Games").unwrap();

    assert!(directory.move_to(directory.id()).is_err());
}

#[test]
fn directory_rehydration_rejects_self_parent_and_inconsistent_timestamps() {
    let id = DirectoryId::new();
    let created_at = Utc::now();
    assert!(matches!(
        Directory::rehydrate(
            id,
            Some(id),
            "self".to_owned(),
            DirectoryKind::default(),
            created_at,
            created_at,
            1,
        ),
        Err(DirectoryError::InvalidFormat {
            field: "directory.parent_id",
            ..
        })
    ));

    assert!(matches!(
        Directory::rehydrate(
            DirectoryId::new(),
            Some(DirectoryId::root()),
            "past".to_owned(),
            DirectoryKind::default(),
            created_at,
            created_at - chrono::Duration::seconds(1),
            1,
        ),
        Err(DirectoryError::InvalidFormat {
            field: "directory.updated_at",
            ..
        })
    ));

    assert!(matches!(
        Directory::rehydrate(
            DirectoryId::new(),
            Some(DirectoryId::root()),
            "invalid revision".to_owned(),
            DirectoryKind::default(),
            created_at,
            created_at,
            0,
        ),
        Err(DirectoryError::InvalidFormat {
            field: "directory.revision",
            ..
        })
    ));
}

#[test]
fn directory_mutations_increment_revision_only_when_state_changes() {
    let mut directory = Directory::new(DirectoryId::root(), "library").unwrap();
    assert_eq!(directory.revision(), 1);

    directory.rename("library").unwrap();
    assert_eq!(directory.revision(), 1);

    directory.rename("media").unwrap();
    assert_eq!(directory.revision(), 2);
    directory.move_to(DirectoryId::new()).unwrap();
    assert_eq!(directory.revision(), 3);
    directory.change_kind(DirectoryKind::try_new("core:collection").unwrap());
    assert_eq!(directory.revision(), 4);
}
