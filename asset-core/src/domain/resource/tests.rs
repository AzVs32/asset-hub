use super::*;
use crate::error::ResourceError;
use serde_json::json;

#[test]
fn new_resource_has_default_lifecycle_state() {
    let resource = Resource::builder(" Design Doc ")
        .with_kind("doc:markdown")
        .build()
        .unwrap();

    assert_eq!(resource.name(), "Design Doc");
    assert_eq!(resource.kind().as_str(), "doc:markdown");
    assert_eq!(resource.status(), ResourceStatus::default());
    assert!(resource.directory().is_root());
    assert!(resource.is_active());
    assert!(!resource.is_archived());
    assert!(!resource.is_deleted());
    assert!(resource.content().is_none());
    assert!(resource.metadata().is_empty());
    assert_eq!(resource.created_at(), resource.updated_at());
}

#[test]
fn resource_builder_accepts_initial_status() {
    let resource = Resource::builder("archived resource")
        .with_status(ResourceStatus::Archived)
        .build()
        .unwrap();

    assert_eq!(resource.status(), ResourceStatus::Archived);
    assert!(resource.is_archived());
}

#[test]
fn resource_status_uses_canonical_boundary_text() {
    assert_eq!(ResourceStatus::Active.as_str(), "active");
    assert_eq!(ResourceStatus::Archived.to_string(), "archived");
    assert_eq!("active".parse(), Ok(ResourceStatus::Active));
    assert_eq!("archived".parse(), Ok(ResourceStatus::Archived));
    assert!("deleted".parse::<ResourceStatus>().is_err());
}

#[test]
fn resource_builder_uses_default_kind() {
    let resource = Resource::builder("unknown resource").build().unwrap();

    assert!(resource.kind().is(ResourceKind::UNKNOWN));
    assert_eq!(resource.kind().as_str(), ResourceKind::UNKNOWN);
}

#[test]
fn resource_can_be_rehydrated_from_snapshot() {
    let id = ResourceId::new();
    let created_at = chrono::Utc::now();
    let updated_at = created_at + chrono::Duration::seconds(5);
    let deleted_at = Some(updated_at + chrono::Duration::seconds(5));

    let resource = Resource::rehydrate(ResourceSnapshot {
        id,
        name: " restored image ".to_string(),
        directory: ResourceDirectory::from_path(" images/raw ").unwrap(),
        kind: ResourceKind::from("core:image"),
        status: ResourceStatus::Archived,
        metadata: ResourceMetadata::default(),
        content: None,
        created_at,
        updated_at,
        deleted_at,
    })
    .unwrap();

    assert_eq!(resource.id(), id);
    assert_eq!(resource.name(), "restored image");
    assert_eq!(resource.directory().path(), "images/raw");
    assert!(resource.kind().is("core:image"));
    assert_eq!(resource.status(), ResourceStatus::Archived);
    assert_eq!(resource.created_at(), created_at);
    assert_eq!(resource.updated_at(), updated_at);
    assert_eq!(resource.deleted_at(), deleted_at);
    assert!(resource.is_deleted());
}

#[test]
fn resource_lifecycle_transitions_update_state() {
    let mut resource = Resource::builder("image")
        .with_kind("core:image")
        .build()
        .unwrap();

    resource.archive().unwrap();
    assert!(resource.is_archived());

    resource.activate().unwrap();
    assert!(resource.is_active());

    resource.soft_delete();
    assert!(resource.is_deleted());
    assert!(!resource.is_active());

    resource.restore();
    assert!(resource.is_active());
    assert!(!resource.is_deleted());
}

#[test]
fn resource_builder_accepts_metadata_and_content() {
    let metadata = ResourceMetadata::builder()
        .with_tags(["rust", "asset"])
        .build()
        .unwrap();
    let checksum = Checksum::sha256("a".repeat(64)).unwrap();
    let content = ResourceContent::builder(42, checksum.clone())
        .with_mime_type(" image/png ")
        .build()
        .unwrap();

    let resource = Resource::builder("image")
        .with_kind("core:image")
        .with_metadata(metadata)
        .with_content(content)
        .build()
        .unwrap();

    let content = resource.content().unwrap();
    assert_eq!(content.mime_type(), Some("image/png"));
    assert_eq!(content.checksum(), &checksum);
    assert_eq!(
        resource
            .metadata()
            .tags()
            .iter()
            .map(ResourceTag::as_str)
            .collect::<Vec<_>>(),
        vec!["rust", "asset"]
    );
}

#[test]
fn resource_path_uniquely_derives_storage_key() {
    let resource = Resource::builder("readme.md")
        .with_directory(ResourceDirectory::from_path("docs/guides").unwrap())
        .with_content(
            ResourceContent::builder(42, Checksum::sha256("a".repeat(64)).unwrap())
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(resource.storage_key().as_str(), "docs/guides/readme.md");
}

#[test]
fn resource_name_must_be_a_single_file_name() {
    for name in [".", "..", "docs/readme.md", "docs\\readme.md"] {
        assert!(matches!(
            Resource::builder(name).build(),
            Err(ResourceError::InvalidFormat {
                field: "resource.name",
                reason: "resource name must be a single file name",
            })
        ));
    }
}

#[test]
fn deleted_resource_rejects_mutations() {
    let mut resource = Resource::builder("image")
        .with_kind("core:image")
        .build()
        .unwrap();
    resource.soft_delete();

    assert_eq!(
        resource.rename("new image"),
        Err(ResourceError::DeletedResource)
    );
    assert_eq!(resource.archive(), Err(ResourceError::DeletedResource));
    assert_eq!(
        resource.attach_content(
            ResourceContent::builder(1, Checksum::sha256("a".repeat(64)).unwrap())
                .build()
                .unwrap(),
        ),
        Err(ResourceError::DeletedResource)
    );
}

#[test]
fn metadata_supports_object_access() {
    let mut metadata = ResourceMetadata::default();

    metadata.add_tag(" rust ").unwrap();

    assert_eq!(metadata.tags()[0].as_str(), "rust");
    assert!(!metadata.is_empty());
}

#[test]
fn metadata_accepts_summary() {
    let metadata = serde_json::from_value::<ResourceMetadata>(json!({
        "summary": {
            "description": "A resource",
            "tags": ["asset"]
        },
        "kind_metadata": null
    }))
    .unwrap();

    assert_eq!(metadata.description(), Some("A resource"));
    assert_eq!(metadata.tags()[0].as_str(), "asset");
}

#[test]
fn metadata_deserialization_rejects_missing_or_unknown_fields() {
    for value in [
        json!(null),
        json!({}),
        json!({"summary": {}}),
        json!({"summary": {"tags": []}}),
        json!({"summary": {"description": null, "tags": []}}),
        json!({
            "summary": {"description": null, "tags": []},
            "legacy": true
        }),
        json!({
            "summary": {"description": null, "tags": []},
            "kind": {}
        }),
    ] {
        assert!(serde_json::from_value::<ResourceMetadata>(value).is_err());
    }
}

#[test]
fn kind_metadata_must_match_resource_kind() {
    let kind_metadata = ResourceKindMetadata::new(
        ResourceKind::from("core:image"),
        1,
        serde_json::Map::from_iter([("width".to_owned(), json!(1200))]),
    )
    .unwrap();
    let metadata = ResourceMetadata::builder()
        .with_kind_metadata(kind_metadata)
        .build()
        .unwrap();

    assert!(
        Resource::builder("image")
            .with_kind("core:image")
            .with_metadata(metadata.clone())
            .build()
            .is_ok()
    );
    assert!(
        Resource::builder("document")
            .with_kind("core:document")
            .with_metadata(metadata)
            .build()
            .is_err()
    );
}

#[test]
fn changing_kind_clears_kind_metadata_but_preserves_summary() {
    let kind_metadata =
        ResourceKindMetadata::new(ResourceKind::from("core:image"), 1, serde_json::Map::new())
            .unwrap();
    let metadata = ResourceMetadata::builder()
        .with_description("cover")
        .with_kind_metadata(kind_metadata)
        .build()
        .unwrap();
    let mut resource = Resource::builder("image")
        .with_kind("core:image")
        .with_metadata(metadata)
        .build()
        .unwrap();

    resource.change_kind("core:document").unwrap();

    assert_eq!(resource.metadata().description(), Some("cover"));
    assert!(resource.metadata().kind_metadata().is_none());
}

#[test]
fn summary_patch_preserves_kind_metadata() {
    let kind_metadata =
        ResourceKindMetadata::new(ResourceKind::from("core:image"), 1, serde_json::Map::new())
            .unwrap();
    let metadata = ResourceMetadata::builder()
        .with_kind_metadata(kind_metadata.clone())
        .build()
        .unwrap();
    let mut resource = Resource::builder("image")
        .with_kind("core:image")
        .with_metadata(metadata)
        .build()
        .unwrap();
    let summary = ResourceSummaryMetadata::new(Some("cover".to_owned()), vec![]).unwrap();

    resource
        .patch_metadata(ResourceMetadataPatch::new().with_summary(summary))
        .unwrap();

    assert_eq!(resource.metadata().description(), Some("cover"));
    assert_eq!(resource.metadata().kind_metadata(), Some(&kind_metadata));
}

#[test]
fn storage_key_rejects_unsafe_paths() {
    assert!(StorageKey::new("assets/image.png").is_ok());
    assert_eq!(
        StorageKey::new(" /absolute/path "),
        Err(ResourceError::InvalidFormat {
            field: "storage.key",
            reason: "absolute paths are not allowed",
        })
    );
    assert_eq!(
        StorageKey::new("assets/../secret"),
        Err(ResourceError::InvalidFormat {
            field: "storage.key",
            reason: "parent path segments are not allowed",
        })
    );
}

#[test]
fn checksum_validates_sha256_format() {
    let value = "a".repeat(64);
    let checksum = Checksum::sha256(&value).unwrap();

    assert_eq!(checksum.kind(), ChecksumKind::Sha256);
    assert_eq!(checksum.value(), value);
    assert!(Checksum::sha256("not-sha256").is_err());
}

#[test]
fn checksum_kind_uses_canonical_boundary_text() {
    assert_eq!(ChecksumKind::Sha256.as_str(), "sha256");
    assert_eq!(ChecksumKind::Sha256.to_string(), "sha256");
    assert_eq!("sha256".parse(), Ok(ChecksumKind::Sha256));
    assert!("md5".parse::<ChecksumKind>().is_err());
}

#[test]
fn content_stores_one_typed_checksum() {
    let checksum = Checksum::sha256("a".repeat(64)).unwrap();
    let content = ResourceContent::builder(1, checksum.clone())
        .build()
        .unwrap();

    assert_eq!(content.checksum(), &checksum);
    assert_eq!(content.checksum().kind(), ChecksumKind::Sha256);
}

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
