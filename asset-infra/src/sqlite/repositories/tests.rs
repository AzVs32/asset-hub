use super::*;
use asset_core::domain::{DefinitionOrigin, DirectoryKindDefinition, StorageKey};
use asset_core::port::{
    DirectoryKindRegistry, DirectoryRelocation, DirectoryRelocationStore, DirectoryRevisionUpdate,
    DirectoryStorage, DirectoryStore, ResourceStore,
};
use asset_core::service::{DirectoryService, DirectoryServices, UpdateDirectory};
use std::collections::HashSet;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
struct TestRepositories {
    resources: Arc<SqliteResourceStore>,
    directories: Arc<SqliteDirectoryStore>,
    pool: SqlitePool,
}

impl Deref for TestRepositories {
    type Target = SqliteResourceStore;

    fn deref(&self) -> &Self::Target {
        self.resources.as_ref()
    }
}

impl AsRef<SqliteResourceStore> for TestRepositories {
    fn as_ref(&self) -> &SqliteResourceStore {
        self.resources.as_ref()
    }
}

impl TestRepositories {
    fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[derive(Default)]
struct TestDirectoryStorage {
    directories: std::sync::Mutex<HashSet<DirectoryPath>>,
}

#[async_trait::async_trait]
impl DirectoryStorage for TestDirectoryStorage {
    async fn directory_exists(&self, directory: &DirectoryPath) -> Result<bool, CoreError> {
        Ok(directory.is_root() || self.directories.lock().unwrap().contains(directory))
    }

    async fn ensure_directory(&self, directory: &DirectoryPath) -> Result<(), CoreError> {
        let mut directories = self.directories.lock().unwrap();
        let mut path = DirectoryPath::root();
        for segment in directory
            .path()
            .split('/')
            .filter(|segment| !segment.is_empty())
        {
            path = path.child(segment)?;
            directories.insert(path.clone());
        }
        Ok(())
    }
    async fn move_directory(
        &self,
        from: &DirectoryPath,
        to: &DirectoryPath,
    ) -> Result<(), CoreError> {
        let mut directories = self.directories.lock().unwrap();
        if directories.contains(to) {
            return Err(CoreError::conflict("directory destination exists"));
        }
        let affected = directories
            .iter()
            .filter(|path| from.contains(path))
            .cloned()
            .collect::<Vec<_>>();
        for path in &affected {
            directories.remove(path);
        }
        for path in affected {
            let suffix = path.path().strip_prefix(from.path()).unwrap();
            directories.insert(DirectoryPath::from_path(format!("{}{suffix}", to.path()))?);
        }
        Ok(())
    }

    async fn delete_empty_directory(&self, directory: &DirectoryPath) -> Result<(), CoreError> {
        let mut directories = self.directories.lock().unwrap();
        if directories
            .iter()
            .any(|candidate| candidate != directory && directory.contains(candidate))
        {
            return Err(CoreError::conflict("directory is not empty"));
        }
        directories.remove(directory);
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

async fn directory_service(repository: Arc<SqliteDirectoryStore>) -> DirectoryService {
    let index = Arc::new(
        crate::directory_index::InMemoryDirectoryIndex::from_directories(
            DirectoryStore::load_all(repository.as_ref()).await.unwrap(),
        )
        .unwrap(),
    );
    DirectoryServices::new(
        repository.clone(),
        index,
        Arc::new(TestDirectoryStorage::default()),
        repository,
        Arc::new(TestDirectoryKinds::default()),
    )
    .directory_service()
}

async fn resource_storage_key(repository: &TestRepositories, resource: &Resource) -> StorageKey {
    let directory = directory_service(repository.directories.clone())
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
    repository.insert(&resource).await.unwrap();
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
        repository.load(&resource.id()).await,
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
    repository.insert(&resource).await.unwrap();
    sqlx::query("UPDATE resources SET name = '..' WHERE id = ?")
        .bind(resource.id().to_string())
        .execute(repository.pool())
        .await
        .unwrap();

    assert!(matches!(
        repository.load(&resource.id()).await,
        Err(CoreError::Repository {
            operation: "resource.rehydrate",
            ..
        })
    ));
}

#[tokio::test]
async fn conditional_save_rejects_a_stale_resource_snapshot() {
    let repository = repository("conditional-save").await;
    let resource = Resource::builder("original").build().unwrap();
    repository.insert(&resource).await.unwrap();

    let expected = resource.revision();
    let mut concurrent = resource.clone();
    concurrent.rename("concurrent").unwrap();
    assert!(
        repository
            .update_if_revision(&concurrent, expected)
            .await
            .unwrap()
    );

    let mut stale = resource.clone();
    stale.rename("stale").unwrap();
    assert!(
        !ResourceStore::update_if_revision(repository.as_ref(), &stale, expected)
            .await
            .unwrap()
    );
    assert_eq!(
        repository
            .load(&resource.id())
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
    let directories = directory_service(repository.directories.clone()).await;
    let located = directories
        .create_with_kind(&DirectoryId::root(), "library", DirectoryKind::default())
        .await
        .unwrap();
    let expected = located.directory().revision();
    let mut stale = located.directory().clone();

    directories
        .update(
            &located.id(),
            UpdateDirectory::new(expected).with_name("current"),
        )
        .await
        .unwrap();
    stale.rename("stale").unwrap();

    assert!(
        !DirectoryStore::update_batch_if_unchanged(
            repository.directories.as_ref(),
            &[DirectoryRevisionUpdate::new(stale, expected).unwrap()]
        )
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
    repository.insert(&resource).await.unwrap();

    let expected = resource.revision();
    let mut concurrent = resource.clone();
    concurrent.rename("concurrent").unwrap();
    assert!(
        repository
            .update_if_revision(&concurrent, expected)
            .await
            .unwrap()
    );

    assert!(
        !repository
            .delete_if_revision(&resource.id(), expected)
            .await
            .unwrap()
    );
    assert!(
        repository
            .delete_if_revision(&resource.id(), concurrent.revision())
            .await
            .unwrap()
    );
    assert!(repository.load(&resource.id()).await.unwrap().is_none());
}

#[tokio::test]
async fn directory_tree_derives_paths_from_stable_ids_after_rename_and_move() {
    let repository = repository("directory-tree").await;
    let directories = directory_service(repository.directories.clone()).await;
    let collections = directories
        .create(&DirectoryId::root(), "Collections")
        .await
        .unwrap();
    let item = directories.create(&collections.id(), "Item").await.unwrap();
    let content = directories.create(&item.id(), "content").await.unwrap();
    let archive = directories
        .create(&DirectoryId::root(), "Archive")
        .await
        .unwrap();
    let resource = Resource::builder("asset.bin")
        .with_directory_id(content.id())
        .build()
        .unwrap();
    repository.insert(&resource).await.unwrap();

    let item_revision = directories
        .find_by_id(&item.id())
        .await
        .unwrap()
        .directory()
        .revision();
    directories
        .update(
            &item.id(),
            UpdateDirectory::new(item_revision).with_name("Renamed"),
        )
        .await
        .unwrap();

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
            &repository.load(&resource.id()).await.unwrap().unwrap(),
        )
        .await
        .as_str(),
        "Collections/Renamed/content/asset.bin"
    );

    let collections_revision = directories
        .find_by_id(&collections.id())
        .await
        .unwrap()
        .directory()
        .revision();
    directories
        .update(
            &collections.id(),
            UpdateDirectory::new(collections_revision).with_parent_id(archive.id()),
        )
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
async fn pending_directory_relocation_completes_after_the_physical_move() {
    let repository = repository("directory-relocation-recovery").await;
    let store = repository.directories.clone();
    let index = Arc::new(
        crate::directory_index::InMemoryDirectoryIndex::from_directories(
            DirectoryStore::load_all(store.as_ref()).await.unwrap(),
        )
        .unwrap(),
    );
    let storage = Arc::new(TestDirectoryStorage::default());
    let services = DirectoryServices::new(
        store.clone(),
        index,
        storage.clone(),
        store.clone(),
        Arc::new(TestDirectoryKinds::default()),
    );
    let directories = services.directory_service();
    let source = directories
        .create(&DirectoryId::root(), "source")
        .await
        .unwrap();
    let current = directories.find_by_id(&source.id()).await.unwrap();
    let expected_revision = current.directory().revision();
    let mut desired = current.directory().clone();
    desired.rename("destination").unwrap();
    let destination = DirectoryPath::from_path("destination").unwrap();
    let relocation = DirectoryRelocation::new(
        source.id(),
        source.path().clone(),
        destination.clone(),
        vec![DirectoryRevisionUpdate::new(desired, expected_revision).unwrap()],
    )
    .unwrap();
    DirectoryRelocationStore::begin(store.as_ref(), &relocation)
        .await
        .unwrap();
    storage
        .move_directory(source.path(), &destination)
        .await
        .unwrap();

    assert_eq!(directories.recover_pending_relocations().await.unwrap(), 1);
    assert_eq!(
        directories.locate_by_id(&source.id()).await.unwrap().path(),
        &destination
    );
    assert!(
        DirectoryRelocationStore::load_pending(store.as_ref())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn directory_store_rejects_cycles() {
    let repository = repository("directory-cycle").await;
    let directories = directory_service(repository.directories.clone()).await;
    let parent = directories
        .create(&DirectoryId::root(), "parent")
        .await
        .unwrap();
    let child = directories.create(&parent.id(), "child").await.unwrap();
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
async fn directory_store_rejects_invalid_persisted_self_parent() {
    let repository = repository("directory-self-parent").await;
    let directory = Directory::new(DirectoryId::root(), "self").unwrap();
    DirectoryStore::insert(repository.directories.as_ref(), &directory)
        .await
        .unwrap();
    sqlx::query("UPDATE directories SET parent_id = id WHERE id = ?")
        .bind(directory.id().to_string())
        .execute(repository.pool())
        .await
        .unwrap();

    assert!(matches!(
        DirectoryStore::load_all(repository.directories.as_ref()).await,
        Err(CoreError::Repository {
            operation: "directory.rehydrate",
            ..
        })
    ));
}

async fn repository(name: &str) -> TestRepositories {
    let database = SqliteDatabase::connect(&unique_temp_path(name).join("asset-hub.sqlite"), 1)
        .await
        .unwrap();
    TestRepositories {
        resources: Arc::new(SqliteResourceStore::new(database.pool().clone())),
        directories: Arc::new(SqliteDirectoryStore::new(database.pool().clone())),
        pool: database.pool().clone(),
    }
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
}
