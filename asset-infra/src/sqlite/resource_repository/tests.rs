use super::*;
use asset_core::domain::{DefinitionOrigin, DirectoryKindDefinition, StorageKey};
use asset_core::port::{
    DirectoryKindRegistry, DirectoryRepository, DirectoryStorage, ResourceRepository,
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
        Self(vec![DirectoryKindDefinition::new(
            DirectoryKind::default(),
            "Directory",
            DefinitionOrigin::builtin_static("test"),
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
            DirectoryRepository::load_all(repository.as_ref())
                .await
                .unwrap(),
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
async fn sqlite_repository_classifies_invalid_resource_state_as_repository_failure() {
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
async fn directory_repository_rejects_a_stale_aggregate_snapshot() {
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
        !DirectoryRepository::save_if_unchanged(repository.as_ref(), &stale, expected)
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
    let resource = Resource::builder("original").build().unwrap();
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
}

#[tokio::test]
async fn directory_tree_derives_paths_from_stable_ids_after_rename_and_move() {
    let repository = repository("directory-tree").await;
    let directories = directory_service(repository.clone()).await;
    let collections = directories
        .ensure_path(&DirectoryPath::from_path("Collections").unwrap())
        .await
        .unwrap();
    let item = directories
        .ensure_path(&DirectoryPath::from_path("Collections/Item").unwrap())
        .await
        .unwrap();
    let content = directories
        .ensure_path(&DirectoryPath::from_path("Collections/Item/content").unwrap())
        .await
        .unwrap();
    let archive = directories
        .ensure_path(&DirectoryPath::from_path("Archive").unwrap())
        .await
        .unwrap();
    let resource = Resource::builder("asset.bin")
        .with_directory_id(content.id())
        .build()
        .unwrap();
    repository.save(&resource).await.unwrap();

    directories.rename(&item.id(), "Renamed").await.unwrap();

    assert_eq!(
        directories
            .locate_by_id(&item.id())
            .await
            .unwrap()
            .path()
            .path(),
        "Collections/Renamed"
    );
    assert_eq!(
        directories
            .locate_by_id(&content.id())
            .await
            .unwrap()
            .path()
            .path(),
        "Collections/Renamed/content"
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
        "Collections/Renamed/content/asset.bin"
    );

    directories
        .move_to(&collections.id(), &archive.id())
        .await
        .unwrap();
    assert_eq!(
        directories
            .locate_by_id(&content.id())
            .await
            .unwrap()
            .path()
            .path(),
        "Archive/Collections/Renamed/content"
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
                UpdateDirectory::new(
                    directories
                        .find_by_id(&parent.id())
                        .await
                        .unwrap()
                        .directory()
                        .revision(),
                )
                .with_parent_id(child.id())
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn directory_repository_rejects_invalid_persisted_self_parent() {
    let repository = repository("directory-self-parent").await;
    let directory = Directory::new(DirectoryId::root(), "self").unwrap();
    DirectoryRepository::insert(repository.as_ref(), &directory)
        .await
        .unwrap();
    sqlx::query("UPDATE directories SET parent_id = id WHERE id = ?")
        .bind(directory.id().to_string())
        .execute(repository.pool())
        .await
        .unwrap();

    assert!(matches!(
        DirectoryRepository::load_all(repository.as_ref()).await,
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
