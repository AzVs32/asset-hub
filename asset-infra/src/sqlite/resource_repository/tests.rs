use super::*;
use asset_core::domain::{Checksum, ResourceContent, StorageKey};
use asset_core::port::{
    DirectoryKindDefinition, DirectoryKindRegistry, DirectoryStorage, DirectoryStore,
    ListResources, ResourceRepository,
};
use asset_core::service::{DirectoryService, UpdateDirectory};
use std::path::PathBuf;
use std::sync::Arc;

struct TestDirectoryStorage;

#[async_trait::async_trait]
impl DirectoryStorage for TestDirectoryStorage {
    async fn ensure_directory(&self, _directory: &DirectoryPath) -> Result<(), CoreError> {
        Ok(())
    }
    async fn move_directory(
        &self,
        _from: &DirectoryPath,
        _to: &DirectoryPath,
    ) -> Result<(), CoreError> {
        Ok(())
    }
}

struct TestDirectoryKinds(Vec<DirectoryKindDefinition>);

impl Default for TestDirectoryKinds {
    fn default() -> Self {
        Self(vec![DirectoryKindDefinition::with_source(
            DirectoryKind::default(),
            "Directory",
            "test",
        )])
    }
}

impl DirectoryKindRegistry for TestDirectoryKinds {
    fn definitions(&self) -> &[DirectoryKindDefinition] {
        &self.0
    }
}

async fn directory_service(repository: Arc<SqliteResourceRepository>) -> DirectoryService {
    let index = Arc::new(
        crate::directory_index::InMemoryDirectoryIndex::from_directories(
            DirectoryStore::load_all(repository.as_ref()).await.unwrap(),
        )
        .unwrap(),
    );
    DirectoryService::new(
        repository,
        index,
        Arc::new(TestDirectoryStorage),
        Arc::new(TestDirectoryKinds::default()),
    )
}

async fn resource_storage_key(
    repository: &Arc<SqliteResourceRepository>,
    resource: &Resource,
) -> StorageKey {
    let directory = directory_service(repository.clone())
        .await
        .locate_by_id(&resource.directory_id())
        .await
        .unwrap();
    StorageKey::from_resource_path(directory.path(), resource.name()).unwrap()
}

#[tokio::test]
async fn sqlite_repository_roundtrips_resource() {
    let repository = repository("roundtrip").await;
    let directories = directory_service(repository.clone()).await;
    let checksum = Checksum::sha256("a".repeat(64)).unwrap();
    let modified_at = chrono::DateTime::parse_from_rfc3339("2026-07-23T03:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let content = ResourceContent::verified(42, checksum.clone())
        .with_mime_type("image/png")
        .with_modified_at(modified_at)
        .build()
        .unwrap();
    let assets = directories
        .ensure_path(&DirectoryPath::from_path("assets").unwrap())
        .await
        .unwrap();
    let resource = Resource::builder("image.png")
        .with_directory_id(assets.id())
        .with_kind(ResourceKind::try_new("core:image").unwrap())
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
    assert_eq!(
        resource_storage_key(&repository, &restored).await.as_str(),
        "assets/image.png"
    );
    assert_eq!(restored_content.size(), 42);
    assert_eq!(restored_content.mime_type(), Some("image/png"));
    assert_eq!(restored_content.checksum(), Some(&checksum));
    assert_eq!(restored_content.modified_at(), Some(modified_at));

    let tag_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_tags WHERE resource_id = ?")
            .bind(resource.id().to_string())
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(tag_rows, 2);
}

#[tokio::test]
async fn sqlite_repository_rejects_invalid_persisted_resource_content() {
    let repository = repository("invalid-content").await;
    let resource = Resource::builder("invalid.bin").build().unwrap();
    repository.save(&resource).await.unwrap();
    sqlx::query(
        r#"
        UPDATE resources
        SET content_json = '{"size":1,"mime_type":null,"verification":{"status":"verified","checksum":{"kind":"sha256","value":"bad"}}}'
        WHERE id = ?
        "#,
    )
    .bind(resource.id().to_string())
    .execute(repository.pool())
    .await
    .unwrap();

    assert!(matches!(
        repository.find_by_id(&resource.id()).await,
        Err(CoreError::Repository {
            operation: "resource.decode_content",
            ..
        })
    ));
}

#[tokio::test]
async fn sqlite_repository_classifies_invalid_resource_snapshot_as_repository_failure() {
    let repository = repository("invalid-resource-snapshot").await;
    let resource = Resource::builder("valid.bin").build().unwrap();
    repository.save(&resource).await.unwrap();
    sqlx::query("UPDATE resources SET name = '..' WHERE id = ?")
        .bind(resource.id().to_string())
        .execute(repository.pool())
        .await
        .unwrap();

    assert!(matches!(
        repository.find_by_id(&resource.id()).await,
        Err(CoreError::Repository {
            operation: "resource.rehydrate",
            ..
        })
    ));
}

#[tokio::test]
async fn sqlite_repository_updates_tags() {
    let repository = repository("tags").await;
    let mut resource = Resource::builder("image")
        .with_kind(ResourceKind::try_new("core:image").unwrap())
        .with_tags(["image", "cover"])
        .build()
        .unwrap();

    repository.save(&resource).await.unwrap();
    assert_eq!(
        repository
            .find_by_path(&DirectoryPath::root(), resource.name())
            .await
            .unwrap()
            .map(|found| found.resource().id()),
        Some(resource.id())
    );
    resource.replace_tags(vec!["document".to_owned()]).unwrap();
    repository.save(&resource).await.unwrap();
    let restored = repository
        .find_by_id(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.tags()[0].as_str(), "document");
}

#[tokio::test]
async fn sqlite_path_lookup_ignores_soft_deleted_resource_and_finds_replacement() {
    let repository = repository("replace-soft-deleted-path").await;
    let directories = directory_service(repository.clone()).await;
    let docs = directories
        .ensure_path(&DirectoryPath::from_path("docs").unwrap())
        .await
        .unwrap();
    let mut deleted = Resource::builder("same-name.txt")
        .with_directory_id(docs.id())
        .build()
        .unwrap();
    repository.save(&deleted).await.unwrap();
    deleted.soft_delete();
    repository.save(&deleted).await.unwrap();

    assert!(
        repository
            .find_by_path(docs.path(), deleted.name())
            .await
            .unwrap()
            .is_none()
    );

    let replacement = Resource::builder("same-name.txt")
        .with_directory_id(docs.id())
        .build()
        .unwrap();
    repository.save(&replacement).await.unwrap();

    assert_eq!(
        repository
            .find_by_path(docs.path(), replacement.name())
            .await
            .unwrap()
            .map(|resource| resource.resource().id()),
        Some(replacement.id())
    );
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
    assert_eq!(page.items[0].resource().id(), rust.id());
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
        .with_kind(ResourceKind::try_new("core:image").unwrap())
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

    let expected = resource.revision();
    let mut concurrent = resource.clone();
    concurrent.rename("concurrent").unwrap();
    repository.save(&concurrent).await.unwrap();

    let mut stale = resource.clone();
    stale.rename("stale").unwrap();
    assert!(
        !ResourceRepository::save_if_unchanged(repository.as_ref(), &stale, expected)
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
async fn directory_store_rejects_a_stale_aggregate_snapshot() {
    let repository = repository("conditional-directory-save").await;
    let directories = directory_service(repository.clone()).await;
    let located = directories
        .create_with_kind(
            &directories.root().await.unwrap(),
            "library",
            DirectoryKind::default(),
        )
        .await
        .unwrap();
    let expected = located.directory().revision();
    let mut stale = located.directory().clone();

    directories.rename(&located.id(), "current").await.unwrap();
    stale.rename("stale").unwrap();

    assert!(
        !DirectoryStore::save_if_unchanged(repository.as_ref(), &stale, expected)
            .await
            .unwrap()
    );
    assert_eq!(
        directories
            .find_by_id(&located.id())
            .await
            .unwrap()
            .directory()
            .name(),
        "current"
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

    let expected = resource.revision();
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
            .remove_if_unchanged(&resource.id(), concurrent.revision())
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

#[tokio::test]
async fn directory_tree_derives_paths_from_stable_ids_after_rename_and_move() {
    let repository = repository("directory-tree").await;
    let directories = directory_service(repository.clone()).await;
    let games = directories
        .ensure_path(&DirectoryPath::from_path("Games").unwrap())
        .await
        .unwrap();
    let title = directories
        .ensure_path(&DirectoryPath::from_path("Games/Title").unwrap())
        .await
        .unwrap();
    let data = directories
        .ensure_path(&DirectoryPath::from_path("Games/Title/data").unwrap())
        .await
        .unwrap();
    let archive = directories
        .ensure_path(&DirectoryPath::from_path("Archive").unwrap())
        .await
        .unwrap();
    let resource = Resource::builder("game.dat")
        .with_directory_id(data.id())
        .build()
        .unwrap();
    repository.save(&resource).await.unwrap();

    directories.rename(&title.id(), "Renamed").await.unwrap();

    assert_eq!(
        directories
            .locate_by_id(&title.id())
            .await
            .unwrap()
            .path()
            .path(),
        "Games/Renamed"
    );
    assert_eq!(
        directories
            .locate_by_id(&data.id())
            .await
            .unwrap()
            .path()
            .path(),
        "Games/Renamed/data"
    );
    assert_eq!(
        resource_storage_key(
            &repository,
            &repository
                .find_by_id(&resource.id())
                .await
                .unwrap()
                .unwrap(),
        )
        .await
        .as_str(),
        "Games/Renamed/data/game.dat"
    );

    directories
        .move_to(&games.id(), &archive.id())
        .await
        .unwrap();
    assert_eq!(
        directories
            .locate_by_id(&data.id())
            .await
            .unwrap()
            .path()
            .path(),
        "Archive/Games/Renamed/data"
    );
}

#[tokio::test]
async fn directory_repository_rejects_cycles() {
    let repository = repository("directory-cycle").await;
    let directories = directory_service(repository.clone()).await;
    let parent = directories
        .ensure_path(&DirectoryPath::from_path("parent").unwrap())
        .await
        .unwrap();
    let child = directories
        .ensure_path(&DirectoryPath::from_path("parent/child").unwrap())
        .await
        .unwrap();
    assert!(
        directories
            .update(
                &parent.id(),
                UpdateDirectory::new().with_parent_id(child.id())
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn directory_repository_rejects_invalid_persisted_self_parent() {
    let repository = repository("directory-self-parent").await;
    let directory = Directory::new(DirectoryId::root(), "self").unwrap();
    DirectoryStore::insert(repository.as_ref(), &directory)
        .await
        .unwrap();
    sqlx::query("UPDATE directories SET parent_id = id WHERE id = ?")
        .bind(directory.id().to_string())
        .execute(repository.pool())
        .await
        .unwrap();

    assert!(matches!(
        DirectoryStore::load_all(repository.as_ref()).await,
        Err(CoreError::Repository {
            operation: "directory.rehydrate",
            ..
        })
    ));
}

async fn repository(name: &str) -> Arc<SqliteResourceRepository> {
    Arc::new(
        SqliteResourceRepository::connect(&unique_temp_path(name).join("asset-hub.sqlite"), 1)
            .await
            .unwrap(),
    )
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
}
