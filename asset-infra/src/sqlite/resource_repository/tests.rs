use super::*;
use asset_core::domain::{Checksum, ResourceContent, StorageKey};
use std::path::PathBuf;

#[tokio::test]
async fn sqlite_repository_roundtrips_resource() {
    let repository = repository("roundtrip").await;
    let checksum = Checksum::sha256("a".repeat(64)).unwrap();
    let content = ResourceContent::builder(StorageKey::new("assets/image.png").unwrap(), 42)
        .with_mime_type("image/png")
        .with_original_filename("image.png")
        .with_checksum(checksum.clone())
        .build()
        .unwrap();
    let resource = Resource::builder("image")
        .with_kind("core:image")
        .with_metadata(
            ResourceMetadata::builder()
                .with_tags(["rust", "asset"])
                .build()
                .unwrap(),
        )
        .with_content(content)
        .build()
        .unwrap();

    repository.save(&resource).await.unwrap();

    let restored = repository
        .find_by_id(&resource.id())
        .await
        .unwrap()
        .unwrap();
    let restored_content = restored.content().unwrap();

    assert_eq!(restored.id(), resource.id());
    assert_eq!(restored.name(), "image");
    assert!(restored.kind().is("core:image"));
    assert_eq!(restored.metadata().tags(), &["rust", "asset"]);
    assert_eq!(restored_content.key().as_str(), "assets/image.png");
    assert_eq!(restored_content.size(), 42);
    assert_eq!(restored_content.mime_type(), Some("image/png"));
    assert_eq!(restored_content.original_filename(), Some("image.png"));
    assert_eq!(
        restored_content.checksums().collect::<Vec<_>>(),
        vec![&checksum]
    );
}

#[tokio::test]
async fn sqlite_repository_upserts_and_removes_resource() {
    let repository = repository("upsert-remove").await;
    let mut resource = Resource::builder("image")
        .with_kind("core:image")
        .build()
        .unwrap();

    repository.save(&resource).await.unwrap();
    resource.rename("renamed image").unwrap();
    repository.save(&resource).await.unwrap();

    let restored = repository
        .find_by_id(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.name(), "renamed image");

    repository.remove(&resource.id()).await.unwrap();
    repository.remove(&resource.id()).await.unwrap();

    assert!(
        repository
            .find_by_id(&resource.id())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn conditional_save_rejects_a_stale_resource_snapshot() {
    let repository = repository("conditional-save").await;
    let resource = Resource::builder("original").build().unwrap();
    repository.save(&resource).await.unwrap();

    let expected = resource.updated_at();
    let mut concurrent = resource.clone();
    concurrent.rename("concurrent").unwrap();
    repository.save(&concurrent).await.unwrap();

    let mut stale = resource.clone();
    stale.rename("stale").unwrap();
    assert!(
        !repository
            .save_if_unchanged(&stale, expected)
            .await
            .unwrap()
    );
    assert_eq!(
        repository
            .find_by_id(&resource.id())
            .await
            .unwrap()
            .unwrap()
            .name(),
        "concurrent"
    );
}

#[tokio::test]
async fn conditional_remove_rejects_a_stale_resource_snapshot() {
    let repository = repository("conditional-remove").await;
    let resource = Resource::builder("original").build().unwrap();
    repository.save(&resource).await.unwrap();

    let expected = resource.updated_at();
    let mut concurrent = resource.clone();
    concurrent.rename("concurrent").unwrap();
    repository.save(&concurrent).await.unwrap();

    assert!(
        !repository
            .remove_if_unchanged(&resource.id(), expected)
            .await
            .unwrap()
    );
    assert!(
        repository
            .remove_if_unchanged(&resource.id(), concurrent.updated_at())
            .await
            .unwrap()
    );
    assert!(
        repository
            .find_by_id(&resource.id())
            .await
            .unwrap()
            .is_none()
    );
}

async fn repository(name: &str) -> SqliteResourceRepository {
    SqliteResourceRepository::connect(&DatabaseConfig {
        sqlite_path: unique_temp_path(name).join("asset-hub.sqlite"),
        max_connections: 1,
    })
    .await
    .unwrap()
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
}
