use super::*;
use crate::domain::DirectoryId;
use crate::error::ResourceError;

#[test]
fn resource_rehydration_rejects_inconsistent_timestamps() {
    let created_at = chrono::Utc::now();

    assert!(matches!(
        Resource::rehydrate(
            ResourceId::new(),
            "image.png".to_owned(),
            DirectoryId::root(),
            ResourceKind::default(),
            None,
            created_at,
            created_at - chrono::Duration::seconds(1),
            1,
            None,
        ),
        Err(ResourceError::InvalidFormat {
            field: "resource.updated_at",
            ..
        })
    ));
}

#[test]
fn resource_content_supports_explicit_pending_and_failed_verification() {
    let pending = ResourceContent::pending(42)
        .with_mime_type("application/octet-stream")
        .build()
        .unwrap();
    assert_eq!(
        pending.verification_status(),
        ContentVerificationStatus::Pending
    );
    assert_eq!(pending.checksum(), None);
    assert_eq!(pending.verification_error(), None);

    let failed = ResourceContent::verification_failed(42, "storage read failed")
        .build()
        .unwrap();
    assert_eq!(
        failed.verification_status(),
        ContentVerificationStatus::Failed
    );
    assert_eq!(failed.checksum(), None);
    assert_eq!(failed.verification_error(), Some("storage read failed"));
}

#[test]
fn resource_name_preserves_spaces_exactly() {
    let resource = Resource::builder(" design  draft 01.md ").build().unwrap();

    assert_eq!(resource.name(), " design  draft 01.md ");
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
        .with_kind(ResourceKind::try_new("core:image").unwrap())
        .build()
        .unwrap();
    resource.soft_delete();

    assert_eq!(
        resource.rename("new image"),
        Err(ResourceError::DeletedResource)
    );
    assert_eq!(
        resource.attach_content(
            ResourceContent::verified(1, Checksum::sha256("a".repeat(64)).unwrap())
                .build()
                .unwrap(),
        ),
        Err(ResourceError::DeletedResource)
    );
}

#[test]
fn storage_key_rejects_unsafe_paths() {
    assert!(StorageKey::new("assets/image.png").is_ok());
    assert_eq!(
        StorageKey::new(" library / design 01.md ")
            .unwrap()
            .as_str(),
        " library / design 01.md "
    );
    assert_eq!(
        StorageKey::new("/absolute/path"),
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
fn storage_key_deserialization_applies_path_validation() {
    assert_eq!(
        serde_json::from_str::<StorageKey>(r#""assets/image.png""#)
            .unwrap()
            .as_str(),
        "assets/image.png"
    );
    assert!(serde_json::from_str::<StorageKey>(r#""/absolute/path""#).is_err());
    assert!(serde_json::from_str::<StorageKey>(r#""assets/../secret""#).is_err());
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
fn checksum_deserialization_applies_value_validation() {
    let valid = serde_json::json!({
        "kind": "sha256",
        "value": "a".repeat(64),
    });
    let checksum: Checksum = serde_json::from_value(valid).unwrap();
    assert_eq!(checksum.value(), "a".repeat(64));

    let invalid = serde_json::json!({
        "kind": "sha256",
        "value": "not-a-checksum",
    });
    assert!(serde_json::from_value::<Checksum>(invalid).is_err());
}

#[test]
fn resource_content_deserialization_applies_builder_validation() {
    let invalid_mime = serde_json::json!({
        "size": 3,
        "mime_type": "   ",
        "verification": { "status": "pending" },
    });
    assert!(serde_json::from_value::<ResourceContent>(invalid_mime).is_err());

    let invalid_failure = serde_json::json!({
        "size": 3,
        "mime_type": null,
        "verification": { "status": "failed", "error": "" },
    });
    assert!(serde_json::from_value::<ResourceContent>(invalid_failure).is_err());

    let invalid_checksum = serde_json::json!({
        "size": 3,
        "mime_type": null,
        "verification": {
            "status": "verified",
            "checksum": { "kind": "sha256", "value": "bad" },
        },
    });
    assert!(serde_json::from_value::<ResourceContent>(invalid_checksum).is_err());
}
