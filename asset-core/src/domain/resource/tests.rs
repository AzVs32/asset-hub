use super::*;
use crate::domain::{DirectoryId, KindIdError};
use crate::error::ResourceError;

#[test]
fn resource_kind_matches_directory_kind_naming_rules() {
    let kind = ResourceKind::try_new("azvs.game:markdown_v2").unwrap();

    assert_eq!(kind.as_str(), "azvs.game:markdown_v2");
    assert!(ResourceKind::try_new(" AzVs.Game:Markdown_V2 ").is_err());
    assert!(ResourceKind::try_new(" Core:Image ").is_err());
}

#[test]
fn resource_kind_rejects_non_namespaced_or_invalid_values() {
    for value in [
        "image",
        ":image",
        "core:",
        "core:image:large",
        "core/image",
        "核心:图片",
    ] {
        assert!(
            ResourceKind::try_new(value).is_err(),
            "`{value}` should be rejected"
        );
    }
}

#[test]
fn resource_kind_is_limited_to_256_characters() {
    let max_length = format!("a:{}", "b".repeat(254));
    let too_long = format!("a:{}", "b".repeat(255));

    assert!(ResourceKind::try_new(max_length).is_ok());
    assert!(matches!(
        ResourceKind::try_new(too_long),
        Err(KindIdError::TooLong { max: 256 })
    ));
}

#[test]
fn resource_kind_serde_requires_canonical_input() {
    assert!(serde_json::from_str::<ResourceKind>(r#"" Core:Image ""#).is_err());
    let kind: ResourceKind = serde_json::from_str(r#""core:image""#).unwrap();

    assert_eq!(kind.as_str(), "core:image");
    assert_eq!(serde_json::to_string(&kind).unwrap(), r#""core:image""#);
    assert!(serde_json::from_str::<ResourceKind>(r#""image""#).is_err());
}

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
fn resource_soft_delete_and_restore_update_state() {
    let mut resource = Resource::builder("image")
        .with_kind(ResourceKind::try_new("core:image").unwrap())
        .build()
        .unwrap();

    resource.soft_delete();
    assert!(resource.is_deleted());
    assert_eq!(resource.revision(), 2);

    resource.restore();
    assert!(!resource.is_deleted());
    assert_eq!(resource.revision(), 3);
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

#[test]
fn checksum_kind_uses_canonical_boundary_text() {
    assert_eq!(ChecksumKind::Sha256.as_str(), "sha256");
    assert_eq!(ChecksumKind::Sha256.to_string(), "sha256");
    assert_eq!("sha256".parse(), Ok(ChecksumKind::Sha256));
    assert!("md5".parse::<ChecksumKind>().is_err());
}
