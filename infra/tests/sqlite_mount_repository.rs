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
