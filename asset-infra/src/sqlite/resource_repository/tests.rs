use super::*;
use asset_core::domain::{Checksum, ResourceContent};
use asset_core::port::ListResources;
use std::path::PathBuf;

#[tokio::test]
async fn sqlite_repository_roundtrips_resource() {
    let repository = repository("roundtrip").await;
    let checksum = Checksum::sha256("a".repeat(64)).unwrap();
    let content = ResourceContent::builder(42, checksum.clone())
        .with_mime_type("image/png")
        .build()
        .unwrap();
    let resource = Resource::builder("image.png")
        .with_directory(ResourceDirectory::from_path("assets").unwrap())
        .with_kind("core:image")
        .with_description("cover")
        .with_tags(["rust", "asset"])
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
    assert_eq!(restored.name(), "image.png");
    assert!(restored.kind().is("core:image"));
    assert_eq!(
        restored
            .tags()
            .iter()
            .map(|tag| tag.as_str())
            .collect::<Vec<_>>(),
        vec!["asset", "rust"]
    );
    assert_eq!(restored.storage_key().as_str(), "assets/image.png");
    assert_eq!(restored_content.size(), 42);
    assert_eq!(restored_content.mime_type(), Some("image/png"));
    assert_eq!(restored_content.checksum(), &checksum);

    let tag_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_tags WHERE resource_id = ?")
            .bind(resource.id().to_string())
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(restored.description(), Some("cover"));
    assert_eq!(tag_rows, 2);
}

#[tokio::test]
async fn sqlite_repository_updates_description_and_tags() {
    let repository = repository("description-tags").await;
    let mut resource = Resource::builder("image")
        .with_kind("core:image")
        .with_description("cover")
        .with_tags(["image", "cover"])
        .build()
        .unwrap();

    repository.save(&resource).await.unwrap();
    assert_eq!(
        repository
            .find_by_path(resource.directory(), resource.name())
            .await
            .unwrap()
            .map(|found| found.id()),
        Some(resource.id())
    );
    resource.set_description(None).unwrap();
    resource.replace_tags(vec!["document".to_owned()]).unwrap();
    repository.save(&resource).await.unwrap();
    let restored = repository
        .find_by_id(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert!(restored.description().is_none());
    assert_eq!(restored.tags()[0].as_str(), "document");
}

#[tokio::test]
async fn sqlite_repository_filters_tags_through_relational_index() {
    let repository = repository("tag-filter").await;
    let rust = Resource::builder("rust")
        .with_tags(["rust", "asset"])
        .build()
        .unwrap();
    let other = Resource::builder("other").build().unwrap();
    repository.save(&rust).await.unwrap();
    repository.save(&other).await.unwrap();

    let page = repository
        .list(&ListResources::new(20, 0).with_tag("rust"))
        .await
        .unwrap();

    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id(), rust.id());
}

#[tokio::test]
async fn sqlite_repository_reuses_tag_dictionary_entries_and_cleans_orphans() {
    let repository = repository("tag-dictionary").await;
    let mut first = Resource::builder("first")
        .with_tags(["shared", "first-only"])
        .build()
        .unwrap();
    let second = Resource::builder("second")
        .with_tags(["shared", "second-only"])
        .build()
        .unwrap();

    repository.save(&first).await.unwrap();
    repository.save(&second).await.unwrap();

    let shared_dictionary_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tags WHERE name = 'shared'")
            .fetch_one(repository.pool())
            .await
            .unwrap();
    let shared_relations: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM resource_tags
        JOIN tags ON tags.id = resource_tags.tag_id
        WHERE tags.name = 'shared'
        "#,
    )
    .fetch_one(repository.pool())
    .await
    .unwrap();
    assert_eq!(shared_dictionary_rows, 1);
    assert_eq!(shared_relations, 2);

    first.replace_tags(vec!["first-only".to_owned()]).unwrap();
    repository.save(&first).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tags WHERE name = 'shared'")
            .fetch_one(repository.pool())
            .await
            .unwrap(),
        1
    );

    repository.remove(&second.id()).await.unwrap();
    let remaining_tags: Vec<String> = sqlx::query_scalar("SELECT name FROM tags ORDER BY name")
        .fetch_all(repository.pool())
        .await
        .unwrap();
    assert_eq!(remaining_tags, vec!["first-only"]);

    repository.remove(&first.id()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tags")
            .fetch_one(repository.pool())
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn sqlite_repository_roundtrips_more_than_sixty_four_tags() {
    let repository = repository("unbounded-tags").await;
    let resource = Resource::builder("tagged")
        .with_tags((0..100).map(|index| format!("tag-{index}")))
        .build()
        .unwrap();

    repository.save(&resource).await.unwrap();
    let restored = repository
        .find_by_id(&resource.id())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(restored.tags().len(), 100);
    assert_eq!(restored.tags()[99].as_str(), "tag-99");
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
    let resource = Resource::builder("original")
        .with_tags(["conditional"])
        .build()
        .unwrap();
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
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tags WHERE name = 'conditional'")
            .fetch_one(repository.pool())
            .await
            .unwrap(),
        1
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
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tags")
            .fetch_one(repository.pool())
            .await
            .unwrap(),
        0
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
