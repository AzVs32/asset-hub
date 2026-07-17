use super::*;
use asset_core::domain::{
    Checksum, ResourceContent, ResourceKind, ResourceKindMetadata, ResourceKindMetadataSet,
    StorageKey,
};
use asset_core::port::ListResources;
use serde_json::json;
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
    assert_eq!(
        restored
            .metadata()
            .tags()
            .iter()
            .map(|tag| tag.as_str())
            .collect::<Vec<_>>(),
        vec!["rust", "asset"]
    );
    assert_eq!(restored_content.key().as_str(), "assets/image.png");
    assert_eq!(restored_content.size(), 42);
    assert_eq!(restored_content.mime_type(), Some("image/png"));
    assert_eq!(restored_content.original_filename(), Some("image.png"));
    assert_eq!(
        restored_content.checksums().collect::<Vec<_>>(),
        vec![&checksum]
    );

    let summary_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM resource_metadata_summaries WHERE resource_id = ?",
    )
    .bind(resource.id().to_string())
    .fetch_one(repository.pool())
    .await
    .unwrap();
    let tag_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_metadata_tags WHERE resource_id = ?")
            .bind(resource.id().to_string())
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(summary_rows, 1);
    assert_eq!(tag_rows, 2);
}

#[tokio::test]
async fn sqlite_repository_roundtrips_three_kind_metadata_layers_without_duplicate_list_items() {
    let repository = repository("kind-metadata-layers").await;
    let file_metadata = kind_metadata("core:file", 1, json!({"media_type": "text/markdown"}));
    let document_metadata = kind_metadata("core:document", 2, json!({"title": "Layered metadata"}));
    let markdown_metadata = kind_metadata("azvs:markdown", 1, json!({"front_matter": true}));
    let expected = ResourceKindMetadataSet::new(vec![
        file_metadata.clone(),
        document_metadata.clone(),
        markdown_metadata.clone(),
    ])
    .unwrap();
    let metadata = ResourceMetadata::builder()
        .with_description("document")
        .with_kind_metadata(file_metadata)
        .with_kind_metadata(document_metadata)
        .with_kind_metadata(markdown_metadata)
        .build()
        .unwrap();
    let resource = Resource::builder("README")
        .with_kind("azvs:markdown")
        .with_metadata(metadata)
        .build()
        .unwrap();

    repository.save(&resource).await.unwrap();
    let restored = repository
        .find_by_id(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.metadata().kind_metadata(), &expected);

    let kind_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_kind_metadata WHERE resource_id = ?")
            .bind(resource.id().to_string())
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(kind_rows, 3);

    let page = repository.list(&ListResources::new(20, 0)).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id(), resource.id());
    assert_eq!(page.items[0].metadata().kind_metadata(), &expected);
}

#[tokio::test]
async fn sqlite_repository_kind_change_keeps_only_common_ancestor_metadata() {
    let repository = repository("kind-metadata-kind-change").await;
    let file_metadata = kind_metadata("core:file", 1, json!({"media_type": "image/png"}));
    let image_metadata = kind_metadata("core:image", 1, json!({"width": 1200, "height": 800}));
    let metadata = ResourceMetadata::builder()
        .with_description("cover")
        .with_kind_metadata(file_metadata.clone())
        .with_kind_metadata(image_metadata)
        .build()
        .unwrap();
    let mut resource = Resource::builder("image")
        .with_kind("core:image")
        .with_metadata(metadata)
        .build()
        .unwrap();

    repository.save(&resource).await.unwrap();
    let video_lineage = [
        ResourceKind::from("core:video"),
        ResourceKind::from("core:file"),
    ];
    resource.change_kind("core:video", &video_lineage).unwrap();
    repository.save(&resource).await.unwrap();

    let restored = repository
        .find_by_id(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.metadata().description(), Some("cover"));
    assert_eq!(
        restored
            .metadata()
            .kind_metadata_for(&ResourceKind::from("core:file")),
        Some(&file_metadata)
    );
    assert!(
        restored
            .metadata()
            .kind_metadata_for(&ResourceKind::from("core:image"))
            .is_none()
    );
    let kind_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_kind_metadata WHERE resource_id = ?")
            .bind(resource.id().to_string())
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(kind_rows, 1);
}

#[tokio::test]
async fn kind_metadata_layer_migration_preserves_legacy_row_and_allows_multiple_kinds() {
    let database_path = unique_temp_path("kind-metadata-migration").join("asset-hub.sqlite");
    std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();

    sqlx::raw_sql(include_str!(
        "../../../migrations/sqlite/0001_create_resources.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO resources (
            id, name, directory, kind, status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("legacy-resource")
    .bind("legacy")
    .bind("")
    .bind("core:image")
    .bind("active")
    .bind("2026-01-01T00:00:00Z")
    .bind("2026-01-01T00:00:00Z")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO resource_kind_metadata (resource_id, kind, schema_version, payload_json) VALUES (?, ?, ?, ?)",
    )
    .bind("legacy-resource")
    .bind("core:image")
    .bind(2_i64)
    .bind(r#"{"width":1200,"height":800}"#)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::raw_sql(include_str!(
        "../../../migrations/sqlite/0004_expand_resource_kind_metadata_layers.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();

    let migrated: (String, i64, String) = sqlx::query_as(
        "SELECT kind, schema_version, payload_json FROM resource_kind_metadata WHERE resource_id = ?",
    )
    .bind("legacy-resource")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(migrated.0, "core:image");
    assert_eq!(migrated.1, 2);
    assert_eq!(migrated.2, r#"{"width":1200,"height":800}"#);

    sqlx::query(
        "INSERT INTO resource_kind_metadata (resource_id, kind, schema_version, payload_json) VALUES (?, ?, ?, ?)",
    )
    .bind("legacy-resource")
    .bind("core:file")
    .bind(1_i64)
    .bind(r#"{"media_type":"image/png"}"#)
    .execute(&pool)
    .await
    .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_kind_metadata WHERE resource_id = ?")
            .bind("legacy-resource")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn sqlite_repository_filters_tags_through_relational_index() {
    let repository = repository("tag-filter").await;
    let rust = Resource::builder("rust")
        .with_metadata(
            ResourceMetadata::builder()
                .with_tags(["rust", "asset"])
                .build()
                .unwrap(),
        )
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

fn kind_metadata(kind: &str, schema_version: u32, data: serde_json::Value) -> ResourceKindMetadata {
    ResourceKindMetadata::new(
        ResourceKind::from(kind),
        schema_version,
        data.as_object().unwrap().clone(),
    )
    .unwrap()
}
