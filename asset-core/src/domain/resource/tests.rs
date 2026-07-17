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
    let content = ResourceContent::builder(StorageKey::new("assets/image.png").unwrap(), 42)
        .with_mime_type(" image/png ")
        .with_original_filename(" image.png ")
        .with_checksum(checksum.clone())
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
    assert_eq!(content.original_filename(), Some("image.png"));
    assert_eq!(content.checksums().collect::<Vec<_>>(), vec![&checksum]);
    assert_eq!(resource.metadata().tags(), &["rust", "asset"]);
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
            ResourceContent::builder(StorageKey::new("a/b").unwrap(), 1)
                .build()
                .unwrap()
        ),
        Err(ResourceError::DeletedResource)
    );
}

#[test]
fn metadata_supports_object_access() {
    let mut metadata = ResourceMetadata::default();

    metadata.add_tag(" rust ").unwrap();

    assert_eq!(
        metadata.schema_version(),
        ResourceMetadata::current_schema_version()
    );
    assert_eq!(metadata.tags(), &["rust"]);
    assert!(!metadata.is_empty());
}

#[test]
fn metadata_accepts_summary() {
    let metadata = ResourceMetadata::from_persisted_value(json!({
        "schema_version": 1,
        "summary": {
            "description": "A resource",
            "tags": ["asset"]
        }
    }))
    .unwrap();

    assert_eq!(metadata.description(), Some("A resource"));
    assert_eq!(metadata.tags(), &["asset"]);
}

#[test]
fn persisted_metadata_rejects_missing_or_legacy_fields() {
    for value in [
        json!(null),
        json!({}),
        json!({"schema_version": 1}),
        json!({"schema_version": 1, "summary": {}}),
        json!({"schema_version": 1, "summary": {"tags": []}}),
        json!({
            "schema_version": 1,
            "summary": {"description": null, "tags": []},
            "legacy": true
        }),
        json!({
            "schema_version": 1,
            "summary": {"description": null, "tags": []},
            "kind": {}
        }),
    ] {
        assert!(ResourceMetadata::from_persisted_value(value).is_err());
    }
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
fn content_rejects_duplicate_checksum_algorithms() {
    let first = Checksum::sha256("a".repeat(64)).unwrap();
    let second = Checksum::sha256("b".repeat(64)).unwrap();
    let result = ResourceContent::builder(StorageKey::new("checksums/file").unwrap(), 1)
        .with_checksums([first, second])
        .build();
    assert!(matches!(
        result,
        Err(ResourceError::InvalidFormat {
            field: "content.checksum",
            ..
        })
    ));
}
