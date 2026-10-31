use ah_vfs::application::mount_runtime::MountRuntime;
mod common;

use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::MountFilter;
use ah_vfs::application::error::MountRepositoryError;
use ah_vfs::application::port::{DriverFuture, MountRepository, Storage};
use ah_vfs::application::service::MountService;
use ah_vfs::domain::{
    ChildMountPolicy, DriverError, DriverInstanceRef, DriverKey, InstanceId, Mount, MountId,
    VirtualPath,
};
use ah_vfs::infrastructure::mount_repository::SqliteMountRepository;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{error::Error, path::Path, sync::Arc};

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    repository: SqliteMountRepository,
    _dir: Option<tempfile::TempDir>,
}

impl Fixture {
    async fn new(memory: bool) -> Result<Self, Box<dyn Error>> {
        if memory {
            Ok(Self {
                repository: SqliteMountRepository::in_memory().await?,
                _dir: None,
            })
        } else {
            let dir = tempfile::tempdir()?;
            let repository =
                SqliteMountRepository::connect(dir.path().join("mounts.sqlite")).await?;
            Ok(Self {
                repository,
                _dir: Some(dir),
            })
        }
    }
}

fn mount(path: &str, driver: &str, instance: &str, enabled: bool) -> Result<Mount, Box<dyn Error>> {
    Ok(Mount::builder()
        .id(MountId::new())
        .virtual_path(VirtualPath::parse(path)?)
        .driver_instance(DriverInstanceRef::new(
            DriverKey::parse(driver)?,
            InstanceId::parse(instance)?,
        ))
        .enabled(enabled)
        .build())
}

fn assert_same(left: &Mount, right: &Mount) {
    assert_eq!(left.id(), right.id());
    assert_eq!(left.virtual_path(), right.virtual_path());
    assert_eq!(left.driver_instance(), right.driver_instance());
    assert_eq!(left.enabled(), right.enabled());
}

#[tokio::test]
async fn crud_and_combined_filters_work_in_file_and_memory_modes() -> TestResult {
    for memory in [false, true] {
        let fixture = Fixture::new(memory).await?;
        let repository = &fixture.repository;
        assert!(repository.list(MountFilter::default()).await?.is_empty());
        assert!(repository.get(MountId::new()).await?.is_none());

        let root = mount("/", "builtin.local", "root", true)?;
        let local = mount("/docs", "builtin.local", "documents", false)?;
        let remote = mount("/archive", "acme.s3", "remote", true)?;
        for entry in [&local, &remote, &root] {
            repository.insert(entry.clone()).await?;
            let restored = repository.get(entry.id()).await?.ok_or("mount missing")?;
            assert_same(&restored, entry);
        }
        let all = repository.list(MountFilter::default()).await?;
        assert_eq!(
            all.iter()
                .map(|entry| entry.virtual_path().as_str())
                .collect::<Vec<_>>(),
            ["/", "/archive", "/docs"]
        );
        assert_eq!(
            repository
                .list(MountFilter::default().enabled(true))
                .await?
                .len(),
            2
        );
        let filtered = repository
            .list(
                MountFilter::default()
                    .driver_key(DriverKey::parse("builtin.local")?)
                    .enabled(false),
            )
            .await?;
        assert_eq!(filtered.len(), 1);
        assert_same(&filtered[0], &local);
        assert!(
            repository
                .list(
                    MountFilter::default()
                        .driver_key(DriverKey::parse("acme.s3")?)
                        .enabled(false)
                )
                .await?
                .is_empty()
        );

        repository.set_enabled(local.id(), true).await?;
        repository.set_enabled(local.id(), true).await?;
        assert!(
            repository
                .get(local.id())
                .await?
                .ok_or("mount missing")?
                .enabled()
        );
        repository.remove(local.id()).await?;
        assert!(repository.get(local.id()).await?.is_none());
        assert!(matches!(
            repository.remove(local.id()).await,
            Err(MountRepositoryError::NotFound(id)) if id == local.id()
        ));
        assert!(matches!(
            repository.set_enabled(local.id(), false).await,
            Err(MountRepositoryError::NotFound(id)) if id == local.id()
        ));
    }
    Ok(())
}

#[tokio::test]
async fn uniqueness_errors_never_replace_existing_data() -> TestResult {
    for memory in [false, true] {
        let fixture = Fixture::new(memory).await?;
        let repository = &fixture.repository;
        let original = mount("/docs", "builtin.local", "original", false)?;
        repository.insert(original.clone()).await?;

        let duplicate_id = Mount::builder()
            .id(original.id())
            .virtual_path(VirtualPath::parse("/other")?)
            .driver_instance(original.driver_instance().clone())
            .enabled(true)
            .build();
        assert!(matches!(
            repository.insert(duplicate_id).await,
            Err(MountRepositoryError::DuplicateId(id)) if id == original.id()
        ));
        assert!(matches!(
            repository.insert(mount("//docs///", "other.driver", "other", true)?).await,
            Err(MountRepositoryError::PathOccupied(path)) if path == *original.virtual_path()
        ));
        let all = repository.list(MountFilter::default()).await?;
        assert_eq!(all.len(), 1);
        assert_same(&all[0], &original);
    }
    Ok(())
}

#[tokio::test]
async fn paths_and_driver_keys_keep_case_unicode_and_quotes() -> TestResult {
    for memory in [false, true] {
        let fixture = Fixture::new(memory).await?;
        let repository = &fixture.repository;
        for path in ["/Docs", "/docs", "/é", "/e\u{301}", "/文档/'报告'"] {
            repository
                .insert(mount(path, "acme.'s3", "实例-😀", true)?)
                .await?;
        }
        assert_eq!(repository.list(MountFilter::default()).await?.len(), 5);
        assert_eq!(
            repository
                .list(MountFilter::default().driver_key(DriverKey::parse("acme.'s3")?))
                .await?
                .len(),
            5
        );
        assert!(
            repository
                .list(MountFilter::default().driver_key(DriverKey::parse("Acme.'s3")?))
                .await?
                .is_empty()
        );
    }
    Ok(())
}

#[tokio::test]
async fn file_storage_survives_reopening_with_stable_ids_and_state() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("mounts.sqlite");
    let original = mount("/documents", "builtin.local", "stable-instance", true)?;
    {
        let repository = SqliteMountRepository::connect(&path).await?;
        repository.insert(original.clone()).await?;
        repository.set_enabled(original.id(), false).await?;
    }
    assert!(path.is_file());
    let repository = SqliteMountRepository::connect(&path).await?;
    let mut expected = original;
    expected.set_enabled(false);
    assert_same(
        &repository
            .get(expected.id())
            .await?
            .ok_or("mount missing")?,
        &expected,
    );
    Ok(())
}

#[tokio::test]
async fn memory_databases_are_isolated_and_new_instances_start_empty() -> TestResult {
    let first = SqliteMountRepository::in_memory().await?;
    let second = SqliteMountRepository::in_memory().await?;
    let entry = mount("/", "builtin.local", "root", true)?;
    first.insert(entry.clone()).await?;
    assert!(second.get(entry.id()).await?.is_none());
    assert!(second.list(MountFilter::default()).await?.is_empty());
    drop(first);
    let reopened = SqliteMountRepository::in_memory().await?;
    assert!(reopened.list(MountFilter::default()).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn injected_pool_shares_the_application_database_and_preserves_other_tables() -> TestResult {
    let dir = tempfile::tempdir()?;
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(dir.path().join("application.sqlite"))
                .create_if_missing(true),
        )
        .await?;
    sqlx::query("CREATE TABLE application_data (value TEXT NOT NULL)")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO application_data VALUES ('retained')")
        .execute(&pool)
        .await?;
    let first = SqliteMountRepository::from_pool(pool.clone()).await?;
    let original = mount("/", "builtin.local", "root", true)?;
    first.insert(original.clone()).await?;

    let second = SqliteMountRepository::from_pool(pool.clone()).await?;
    assert_same(
        &second.get(original.id()).await?.ok_or("mount missing")?,
        &original,
    );
    let value: String = sqlx::query_scalar("SELECT value FROM application_data")
        .fetch_one(&pool)
        .await?;
    assert_eq!(value, "retained");

    pool.close().await;
    assert!(matches!(
        first.get(original.id()).await,
        Err(MountRepositoryError::Backend(_))
    ));
    assert!(matches!(
        second.list(MountFilter::default()).await,
        Err(MountRepositoryError::Backend(_))
    ));
    assert!(matches!(
        first
            .insert(mount("/other", "builtin.local", "other", true)?)
            .await,
        Err(MountRepositoryError::Backend(_))
    ));
    assert!(matches!(
        first.remove(original.id()).await,
        Err(MountRepositoryError::Backend(_))
    ));
    assert!(matches!(
        first.set_enabled(original.id(), false).await,
        Err(MountRepositoryError::Backend(_))
    ));
    Ok(())
}

#[tokio::test]
async fn invalid_stored_values_are_reported_without_normalizing_or_skipping_rows() -> TestResult {
    for field in ["id", "path", "driver", "instance"] {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let repository = SqliteMountRepository::from_pool(pool.clone()).await?;
        let id = MountId::new();
        let stored_id = if field == "id" {
            "broken-id".into()
        } else {
            id.to_string()
        };
        let path = if field == "path" { "//docs/" } else { "/docs" };
        let driver = if field == "driver" {
            ""
        } else {
            "builtin.local"
        };
        let instance = if field == "instance" {
            "invalid instance"
        } else {
            "documents"
        };
        sqlx::query("INSERT INTO vfs_mounts VALUES (?, ?, ?, ?, 1)")
            .bind(stored_id)
            .bind(path)
            .bind(driver)
            .bind(instance)
            .execute(&pool)
            .await?;
        assert!(matches!(
            repository.list(MountFilter::default()).await,
            Err(MountRepositoryError::InvalidData(_))
        ));
        if field != "id" {
            assert!(matches!(
                repository.get(id).await,
                Err(MountRepositoryError::InvalidData(_))
            ));
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vfs_mounts")
            .fetch_one(&pool)
            .await?;
        assert_eq!(count, 1);
    }
    Ok(())
}

#[tokio::test]
async fn schema_rejects_invalid_enabled_values_and_file_open_errors_are_returned() -> TestResult {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    let repository = SqliteMountRepository::from_pool(pool.clone()).await?;
    assert!(
        sqlx::query("INSERT INTO vfs_mounts VALUES (?, '/', 'builtin.local', 'root', 2)")
            .bind(MountId::new().to_string())
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(repository.list(MountFilter::default()).await?.is_empty());
    for path in [Path::new(""), Path::new(":memory:")] {
        assert!(matches!(
            SqliteMountRepository::connect(path).await,
            Err(MountRepositoryError::Backend(_))
        ));
    }
    let dir = tempfile::tempdir()?;
    assert!(matches!(
        SqliteMountRepository::connect(dir.path()).await,
        Err(MountRepositoryError::Backend(_))
    ));
    assert!(matches!(
        SqliteMountRepository::connect(dir.path().join("missing/db.sqlite")).await,
        Err(MountRepositoryError::Backend(_))
    ));
    assert!(!dir.path().join("missing").exists());
    Ok(())
}

struct UnopenedDriver(DriverInfo);

impl common::Backend for UnopenedDriver {
    fn registry(self: Arc<Self>) -> Arc<dyn ah_vfs::application::port::InstanceRegistry> {
        common::static_registry(&[])
    }

    fn info(&self) -> &DriverInfo {
        &self.0
    }
    fn open(
        &self,
        context: ah_vfs::application::port::OpenContext,
    ) -> DriverFuture<'_, Arc<dyn Storage>> {
        let id = context.instance_id().clone();
        Box::pin(async move { Err(DriverError::InstanceNotFound(id)) })
    }
}

#[tokio::test]
async fn mount_service_initializes_and_restores_using_real_sqlite() -> TestResult {
    for memory in [false, true] {
        let fixture = Fixture::new(memory).await?;
        let repository = Arc::new(fixture.repository);
        let mut catalog = DriverCatalog::new();
        common::register(
            &mut catalog,
            common::managed(Arc::new(UnopenedDriver(
                DriverInfo::builder()
                    .capabilities(ah_vfs::domain::StorageCapabilities::default())
                    .key(DriverKey::parse("builtin.local")?)
                    .name("Local test driver".into())
                    .version(semver::Version::new(0, 1, 0))
                    .child_mount_policy(ChildMountPolicy::Allow)
                    .build(),
            ))),
        )?;
        let service = MountService::new(Arc::new(MountRuntime::new(
            repository.clone(),
            Arc::new(catalog),
        )));
        let reference = DriverInstanceRef::new(
            DriverKey::parse("builtin.local")?,
            InstanceId::parse("root")?,
        );
        let root = service.initialize_root(reference.clone()).await?;
        repository
            .insert(mount("/docs", "builtin.local", "documents", true)?)
            .await?;
        repository.set_enabled(root, false).await?;
        assert_eq!(service.initialize_root(reference).await?, root);
        assert!(!repository.get(root).await?.ok_or("root missing")?.enabled());
        assert_eq!(repository.list(MountFilter::default()).await?.len(), 2);
    }
    Ok(())
}
