use super::*;
use crate::domain::{DirectoryId, DirectoryPath, DirectoryRef};
use crate::error::ResourceError;

fn directory(path: &str) -> DirectoryRef {
    let path = DirectoryPath::from_path(path).unwrap();
    if path.is_root() {
        DirectoryRef::root()
    } else {
        DirectoryRef::new(DirectoryId::new(), path)
    }
}

#[test]
fn resource_kind_matches_directory_kind_naming_rules() {
    let kind = ResourceKind::try_new(" AzVs.Game:Markdown_V2 ").unwrap();

    assert_eq!(kind.as_str(), "azvs.game:markdown_v2");
    assert_eq!(
        ResourceKind::try_new(" Core:Image ").unwrap().as_str(),
        "core:image"
    );
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
        Err(ResourceError::TooLong {
            field: "resource.kind",
            max: 256,
        })
    ));
}

#[test]
fn resource_kind_serde_normalizes_and_validates_input() {
    let kind: ResourceKind = serde_json::from_str(r#"" Core:Image ""#).unwrap();

    assert_eq!(kind.as_str(), "core:image");
    assert_eq!(serde_json::to_string(&kind).unwrap(), r#""core:image""#);
    assert!(serde_json::from_str::<ResourceKind>(r#""image""#).is_err());
}

#[test]
fn new_resource_has_default_lifecycle_state() {
    let resource = Resource::builder(" Design Doc ")
        .with_kind(ResourceKind::try_new("doc:markdown").unwrap())
        .build()
        .unwrap();

    assert_eq!(resource.name(), " Design Doc ");
    assert_eq!(resource.kind().as_str(), "doc:markdown");
    assert_eq!(resource.status(), ResourceStatus::default());
    assert!(resource.directory().id().is_root());
    assert!(resource.is_active());
    assert!(!resource.is_archived());
    assert!(!resource.is_deleted());
    assert!(resource.content().is_none());
    assert!(resource.tags().is_empty());
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
    let resource = Resource::builder("generic resource").build().unwrap();

    assert!(resource.kind().is(ResourceKind::DEFAULT));
    assert_eq!(resource.kind().as_str(), ResourceKind::DEFAULT);
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
        directory: directory(" images/raw "),
        kind: ResourceKind::try_new("core:image").unwrap(),
        status: ResourceStatus::Archived,
        tags: vec![" image ".to_owned()],
        content: None,
        created_at,
        updated_at,
        deleted_at,
    })
    .unwrap();

    assert_eq!(resource.id(), id);
    assert_eq!(resource.name(), " restored image ");
    assert_eq!(resource.directory().path().path(), " images/raw ");
    assert!(resource.kind().is("core:image"));
    assert_eq!(resource.status(), ResourceStatus::Archived);
    assert_eq!(resource.tags()[0].as_str(), "image");
    assert_eq!(resource.created_at(), created_at);
    assert_eq!(resource.updated_at(), updated_at);
    assert_eq!(resource.deleted_at(), deleted_at);
    assert!(resource.is_deleted());
}

#[test]
fn resource_lifecycle_transitions_update_state() {
    let mut resource = Resource::builder("image")
        .with_kind(ResourceKind::try_new("core:image").unwrap())
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
fn resource_builder_accepts_tags_and_content() {
    let checksum = Checksum::sha256("a".repeat(64)).unwrap();
    let modified_at = chrono::DateTime::parse_from_rfc3339("2026-07-23T03:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let content = ResourceContent::builder(42, checksum.clone())
        .with_mime_type(" image/png ")
        .with_modified_at(modified_at)
        .build()
        .unwrap();

    let resource = Resource::builder("image")
        .with_kind(ResourceKind::try_new("core:image").unwrap())
        .with_tags(["rust", "asset"])
        .with_content(content)
        .build()
        .unwrap();

    let content = resource.content().unwrap();
    assert_eq!(content.mime_type(), Some("image/png"));
    assert_eq!(content.checksum(), &checksum);
    assert_eq!(content.modified_at(), Some(modified_at));
    assert_eq!(
        resource
            .tags()
            .iter()
            .map(ResourceTag::as_str)
            .collect::<Vec<_>>(),
        vec!["asset", "rust"]
    );
}

#[test]
fn resource_path_uniquely_derives_storage_key() {
    let resource = Resource::builder("readme.md")
        .with_directory(directory("docs/guides"))
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
fn resource_path_preserves_spaces_exactly() {
    let resource = Resource::builder(" design  draft 01.md ")
        .with_directory(directory(" library / project A "))
        .build()
        .unwrap();

    assert_eq!(resource.name(), " design  draft 01.md ");
    assert_eq!(resource.directory().path().path(), " library / project A ");
    assert_eq!(
        resource.storage_key().as_str(),
        " library / project A / design  draft 01.md "
    );
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
fn tags_can_be_replaced() {
    let mut resource = Resource::builder("image")
        .with_kind(ResourceKind::try_new("core:image").unwrap())
        .with_tags([" image ", "cover", "image"])
        .build()
        .unwrap();

    assert_eq!(resource.tags().len(), 2);
    assert_eq!(resource.tags()[0].as_str(), "cover");
    assert_eq!(resource.tags()[1].as_str(), "image");
    resource.replace_tags(vec!["document".to_owned()]).unwrap();

    assert_eq!(resource.tags()[0].as_str(), "document");
}

#[test]
fn resource_tags_do_not_have_a_count_limit() {
    let tags = (0..100).map(|index| format!("tag-{index}"));
    let resource = Resource::builder("tagged").with_tags(tags).build().unwrap();

    assert_eq!(resource.tags().len(), 100);
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
