use super::*;
use crate::sqlite::SqliteResourceRepository;
use asset_core::domain::{Checksum, Resource};
use asset_core::port::ResourceRepository;

#[tokio::test]
async fn pending_replacement_roundtrips_and_is_unique_per_resource() {
    let path = std::env::temp_dir()
        .join(format!(
            "asset-hub-content-replacement-{}",
            uuid::Uuid::now_v7()
        ))
        .join("asset-hub.sqlite");
    let resources = SqliteResourceRepository::connect(&path, 1).await.unwrap();
    let repository = SqliteResourceContentReplacementRepository::new(resources.pool().clone());
    let content = ResourceContent::verified(3, Checksum::sha256("a".repeat(64)).unwrap())
        .with_mime_type("text/plain")
        .build()
        .unwrap();
    let resource = Resource::builder("note.txt")
        .with_content(content.clone())
        .build()
        .unwrap();
    resources.save(&resource).await.unwrap();
    let pending = ResourceContentReplacement::new(
        resource.id(),
        resource.revision(),
        StorageKey::new("note.txt").unwrap(),
        StorageKey::new(".asset-hub/uploads/replacement-test").unwrap(),
        StorageKey::new(".asset-hub/content-backups/replacement-test").unwrap(),
        content,
    )
    .unwrap();

    repository.save(&pending).await.unwrap();
    assert!(repository.save(&pending).await.is_err());
    assert_eq!(
        repository.list_pending().await.unwrap(),
        vec![pending.clone()]
    );

    repository.remove(&pending.id()).await.unwrap();
    repository.remove(&pending.id()).await.unwrap();
    assert!(repository.list_pending().await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_persisted_replacement_content_is_rejected() {
    let path = std::env::temp_dir()
        .join(format!(
            "asset-hub-invalid-content-replacement-{}",
            uuid::Uuid::now_v7()
        ))
        .join("asset-hub.sqlite");
    let resources = SqliteResourceRepository::connect(&path, 1).await.unwrap();
    let repository = SqliteResourceContentReplacementRepository::new(resources.pool().clone());
    let content = ResourceContent::pending(3).build().unwrap();
    let resource = Resource::builder("note.txt")
        .with_content(content.clone())
        .build()
        .unwrap();
    resources.save(&resource).await.unwrap();
    let pending = ResourceContentReplacement::new(
        resource.id(),
        resource.revision(),
        StorageKey::new("note.txt").unwrap(),
        StorageKey::new(".asset-hub/uploads/invalid-replacement").unwrap(),
        StorageKey::new(".asset-hub/content-backups/invalid-replacement").unwrap(),
        content,
    )
    .unwrap();
    repository.save(&pending).await.unwrap();
    sqlx::query(
        r#"
        UPDATE resource_content_replacements
        SET replacement_content_json = '{"size":3,"mime_type":" ","verification":{"status":"pending"}}'
        WHERE id = ?
        "#,
    )
    .bind(pending.id().to_string())
    .execute(resources.pool())
    .await
    .unwrap();

    assert!(matches!(
        repository.list_pending().await,
        Err(CoreError::Repository {
            operation: "content_replacement.decode_content",
            ..
        })
    ));
}
