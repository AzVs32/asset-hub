mod content;
mod kind;
mod metadata;
mod resource;
mod status;

pub use content::{Checksum, ChecksumKind, ResourceContent, ResourceContentBuilder, StorageKey};
pub use kind::ResourceKind;
pub use metadata::{ResourceMetadata, ResourceMetadataBuilder};
pub use resource::{Resource, ResourceBuilder, ResourceId, ResourceSnapshot};
pub use status::ResourceStatus;

use crate::error::ResourceError;

/// 归一化并校验资源领域中的必填文本字段。
///
/// 校验规则：
/// - 去除首尾空白后不能为空，否则返回 `ResourceError::Blank`。
/// - 文本长度按 Unicode 字符数计算，超过 `max` 时返回 `ResourceError::TooLong`。
/// - 不允许包含控制字符，例如换行、制表符或不可见控制码，否则返回
///   `ResourceError::InvalidFormat`。
///
/// `field` 用于标识具体出错的领域字段，最终会原样出现在错误对象中，方便调用方定位
/// 是哪个属性没有通过校验。
fn normalize_required_text(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, ResourceError> {
    let value = value.trim();

    if value.is_empty() {
        return Err(ResourceError::Blank { field });
    }

    if value.chars().count() > max {
        return Err(ResourceError::TooLong { field, max });
    }

    if value.chars().any(char::is_control) {
        return Err(ResourceError::InvalidFormat {
            field,
            reason: "control characters are not allowed",
        });
    }

    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
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
            kind: ResourceKind::from("asset:image"),
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
        assert!(resource.kind().is("asset:image"));
        assert_eq!(resource.status(), ResourceStatus::Archived);
        assert_eq!(resource.created_at(), created_at);
        assert_eq!(resource.updated_at(), updated_at);
        assert_eq!(resource.deleted_at(), deleted_at);
        assert!(resource.is_deleted());
    }

    #[test]
    fn resource_lifecycle_transitions_update_state() {
        let mut resource = Resource::builder("image")
            .with_kind("asset:image")
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
            .with_attribute("source", json!("test"))
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
            .with_kind("asset:image")
            .with_metadata(metadata)
            .with_content(content)
            .build()
            .unwrap();

        let content = resource.content().unwrap();
        assert_eq!(content.mime_type(), Some("image/png"));
        assert_eq!(content.original_filename(), Some("image.png"));
        assert_eq!(content.checksums(), &[checksum]);
        assert_eq!(resource.metadata().tags(), &["rust", "asset"]);
        assert_eq!(
            resource.metadata().attribute("source"),
            Some(&json!("test"))
        );
    }

    #[test]
    fn deleted_resource_rejects_mutations() {
        let mut resource = Resource::builder("image")
            .with_kind("asset:image")
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
        metadata
            .insert_attribute(" language ", json!("rust"))
            .unwrap();

        assert_eq!(
            metadata.schema_version(),
            ResourceMetadata::current_schema_version()
        );
        assert_eq!(metadata.tags(), &["rust"]);
        assert_eq!(metadata.attribute("language"), Some(&json!("rust")));
        assert!(!metadata.is_empty());
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
}
