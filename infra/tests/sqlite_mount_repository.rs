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
async fn transactions_update_definitions_by_id_and_preserve_other_records() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("mounts.sqlite");
    let repository = SqliteMountRepository::open(&file).await.unwrap();
    let assets = mount("/assets", true);
    let other = mount("/other", true);
    repository.insert(&assets).await.unwrap();
    repository.insert(&other).await.unwrap();

    let update = Mount::new(
        assets.id(),
        assets.virtual_path().clone(),
        DriverKind::try_from("memory").unwrap(),
        DriverPath::new("another-root"),
        false,
    );
    let mut transaction = repository.begin().await.unwrap();
    assert!(transaction.update(&update).await.unwrap());
    transaction.commit().await.unwrap();
    let reopened = SqliteMountRepository::open(&file).await.unwrap();
    let persisted = reopened.get(assets.id()).await.unwrap().unwrap();
    assert_eq!(persisted.driver(), update.driver());
    assert_eq!(persisted.driver_path(), update.driver_path());
    assert!(!persisted.enabled());
    assert_eq!(reopened.get(other.id()).await.unwrap(), Some(other.clone()));
    assert_eq!(reopened.list().await.unwrap().len(), 2);

    let conflict = Mount::new(
        assets.id(),
        other.virtual_path().clone(),
        assets.driver().clone(),
        assets.driver_path().clone(),
        true,
    );
    let before = repository.list().await.unwrap();
    let mut transaction = repository.begin().await.unwrap();
    assert!(matches!(
        transaction.update(&conflict).await,
        Err(VfsError::Mount(MountError::DuplicatePath(_)))
    ));
    let missing = mount("/missing", false);
    assert!(!transaction.update(&missing).await.unwrap());
    transaction.commit().await.unwrap();
    assert_eq!(repository.list().await.unwrap(), before);
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

    let same_path = mount("/assets", true);
    assert!(matches!(
        second_repository.insert(&same_path).await,
        Err(VfsError::Mount(MountError::DuplicatePath(path))) if path == *first.virtual_path()
    ));
    assert_eq!(first_repository.list().await.unwrap(), vec![first]);
}

#[tokio::test]
async fn disabled_paths_can_repeat_but_enabled_paths_are_unique_in_sqlite() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("mounts.sqlite");
    let first = SqliteMountRepository::open(&file).await.unwrap();
    let second = SqliteMountRepository::open(&file).await.unwrap();
    let active = mount("/shared", true);
    let disabled = [mount("/shared", false), mount("/shared", false)];
    for definition in &disabled {
        first.insert(definition).await.unwrap();
    }
    let competing = mount("/shared", true);
    let (left, right) = tokio::join!(first.insert(&active), second.insert(&competing));
    assert_ne!(left.is_ok(), right.is_ok());
    let error = left.err().or_else(|| right.err()).unwrap();
    assert!(matches!(
        error,
        VfsError::Mount(MountError::DuplicatePath(_))
    ));
    let saved = first.list().await.unwrap();
    assert_eq!(saved.len(), 3);
    assert!(saved[0].enabled());
    assert_eq!(saved.iter().filter(|mount| mount.enabled()).count(), 1);

    let mut transaction = first.begin().await.unwrap();
    let mut enabled_copy = disabled[0].clone();
    enabled_copy.enable();
    assert!(matches!(
        transaction.update(&enabled_copy).await,
        Err(VfsError::Mount(MountError::DuplicatePath(_)))
    ));
    drop(transaction);
    assert_eq!(
        first.get(disabled[0].id()).await.unwrap(),
        Some(disabled[0].clone())
    );

    let pool = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&file))
        .await
        .unwrap();
    let result = sqlx::query("INSERT INTO mounts VALUES (?1, '/shared', 'local', 'root', 1)")
        .bind(MountId::new().to_string())
        .execute(&pool)
        .await;
    assert!(
        result
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .is_unique_violation()
    );
    pool.close().await;
}

#[tokio::test]
async fn transactions_roll_back_when_dropped() {
    let directory = tempdir().unwrap();
    let repository = SqliteMountRepository::open(directory.path().join("mounts.sqlite"))
        .await
        .unwrap();
    let active = mount("/pending", true);
    let mut transaction = repository.begin().await.unwrap();
    transaction.insert(&active).await.unwrap();
    drop(transaction);
    assert!(repository.get(active.id()).await.unwrap().is_none());
    repository.insert(&active).await.unwrap();
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
