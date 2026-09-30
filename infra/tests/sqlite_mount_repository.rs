use asset_infra::repository::SqliteMountRepository;
use asset_vfs::driver::{DriverKind, DriverPath};
use asset_vfs::error::VfsError;
use asset_vfs::mount::{Mount, MountError, MountId, MountRepository};
use asset_vfs::namespace::VirtualPath;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use tempfile::tempdir;

fn mount(path: &str, enabled: bool) -> Mount {
    Mount::new(
        MountId::new(),
        VirtualPath::try_from(path).unwrap(),
        DriverKind::try_from("local").unwrap(),
        DriverPath::new("driver-specific-root"),
        enabled,
    )
}

#[tokio::test]
async fn upsert_by_path_preserves_identity_and_accepts_arbitrary_mount_fields() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("mounts.sqlite");
    let repository = SqliteMountRepository::open(&file).await.unwrap();
    let assets = mount("/assets", true);
    let other = mount("/other", true);
    repository.upsert_by_path(&assets).await.unwrap();
    repository.insert(&other).await.unwrap();

    // The incoming ID is ignored for an existing path, even if used elsewhere.
    let update = Mount::new(
        other.id(),
        assets.virtual_path().clone(),
        DriverKind::try_from("memory").unwrap(),
        DriverPath::new("another-root"),
        false,
    );
    repository.upsert_by_path(&update).await.unwrap();
    let reopened = SqliteMountRepository::open(&file).await.unwrap();
    let persisted = reopened.get(assets.id()).await.unwrap().unwrap();
    assert_eq!(persisted.driver(), update.driver());
    assert_eq!(persisted.driver_path(), update.driver_path());
    assert!(!persisted.enabled());
    assert_eq!(reopened.get(other.id()).await.unwrap(), Some(other));
    assert_eq!(reopened.list().await.unwrap().len(), 2);

    let conflict = Mount::new(
        assets.id(),
        VirtualPath::try_from("/new").unwrap(),
        assets.driver().clone(),
        assets.driver_path().clone(),
        true,
    );
    let before = repository.list().await.unwrap();
    assert!(matches!(
        repository.upsert_by_path(&conflict).await,
        Err(VfsError::Mount(MountError::DuplicateId(id))) if id == assets.id()
    ));
    assert_eq!(repository.list().await.unwrap(), before);
}

#[tokio::test]
async fn concurrent_upserts_keep_one_definition_per_path() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("mounts.sqlite");
    let first = SqliteMountRepository::open(&file).await.unwrap();
    let second = SqliteMountRepository::open(&file).await.unwrap();
    let a = mount("/shared", true);
    let b = mount("/shared", false);
    let (left, right) = tokio::join!(first.upsert_by_path(&a), second.upsert_by_path(&b));
    left.unwrap();
    right.unwrap();
    let mounts = first.list().await.unwrap();
    assert_eq!(mounts.len(), 1);
    assert!(mounts[0].id() == a.id() || mounts[0].id() == b.id());
    let id = mounts[0].id();
    second.upsert_by_path(&b).await.unwrap();
    let saved = first.get(id).await.unwrap().unwrap();
    assert!(!saved.enabled());
}

#[tokio::test]
async fn mounts_round_trip_across_reopened_database_and_remove_reports_presence() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("mounts.sqlite");
    let assets = mount("/assets", true);
    let archive = mount("/assets/archive", false);

    {
        let repository = SqliteMountRepository::open(&file).await.unwrap();
        let repository: &dyn MountRepository = &repository;
        repository.insert(&archive).await.unwrap();
        repository.insert(&assets).await.unwrap();
        assert_eq!(
            repository.list().await.unwrap(),
            vec![assets.clone(), archive.clone()]
        );
    }

    let repository = SqliteMountRepository::open(&file).await.unwrap();
    assert_eq!(
        repository.get(assets.id()).await.unwrap(),
        Some(assets.clone())
    );
    assert_eq!(
        repository.get(archive.id()).await.unwrap(),
        Some(archive.clone())
    );
    assert!(repository.remove(assets.id()).await.unwrap());
    assert!(!repository.remove(assets.id()).await.unwrap());
    assert_eq!(repository.get(assets.id()).await.unwrap(), None);
    assert_eq!(repository.list().await.unwrap(), vec![archive]);
}

#[tokio::test]
async fn duplicate_ids_and_paths_are_rejected_across_connections() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("mounts.sqlite");
    let first_repository = SqliteMountRepository::open(&file).await.unwrap();
    let second_repository = SqliteMountRepository::open(&file).await.unwrap();
    let first = mount("/assets", true);
    first_repository.insert(&first).await.unwrap();

    let same_id = Mount::new(
        first.id(),
        VirtualPath::try_from("/other").unwrap(),
        first.driver().clone(),
        first.driver_path().clone(),
        true,
    );
    assert!(matches!(
        second_repository.insert(&same_id).await,
        Err(VfsError::Mount(MountError::DuplicateId(id))) if id == first.id()
    ));

    let same_path = mount("/assets", false);
    assert!(matches!(
        second_repository.insert(&same_path).await,
        Err(VfsError::Mount(MountError::DuplicatePath(path))) if path == *first.virtual_path()
    ));
    assert_eq!(first_repository.list().await.unwrap(), vec![first]);
}

#[tokio::test]
async fn invalid_stored_domain_value_is_reported() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("mounts.sqlite");
    let repository = SqliteMountRepository::open(&file).await.unwrap();
    let id = MountId::new();
    let pool = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&file))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mounts (id, virtual_path, driver_kind, driver_path, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(id.to_string())
    .bind("/bad//path")
    .bind("local")
    .bind("root")
    .bind(1_i64)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    assert!(matches!(
        repository.get(id).await,
        Err(VfsError::Mount(MountError::InvalidStoredMount {
            column: "virtual_path"
        }))
    ));
}
