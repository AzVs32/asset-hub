#![cfg(target_os = "linux")]
use ah_vfs::application::driver_error::LifecycleFailure;
mod common;

use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::dto::{InstanceQuery, RemoveInstanceOutcome};
use ah_vfs::application::port::Driver;
use ah_vfs::domain::InstanceId;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::error::Error;
use vfs_driver_local_linux::LinuxLocalDriver;

type TestResult = Result<(), Box<dyn Error>>;

#[tokio::test]
async fn local_instances_reopen_and_deleted_dirs_fail_to_open() -> TestResult {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("本地目录");
    std::fs::create_dir(&source)?;
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("local.sqlite"))
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    let driver = LinuxLocalDriver::new()?;
    let registry = common::registry(&driver, pool.clone()).await?;
    assert_eq!(driver.info().key().as_str(), "local");
    let instance = registry.create(common::local_config(&source)?).await?;
    assert_eq!(
        instance.details["root_path"].as_str(),
        std::fs::canonicalize(&source)?.to_str()
    );
    drop(driver);

    let reopened = LinuxLocalDriver::new()?;
    let restored_registry = common::registry(&reopened, pool.clone()).await?;
    let restored = restored_registry
        .get(instance.id.clone())
        .await?
        .ok_or("instance missing")?;
    assert_eq!(
        restored.details["root_path"].as_str(),
        instance.details["root_path"].as_str()
    );
    let storage = reopened
        .open(restored_registry.open_context(instance.id.clone()).await?)
        .await?;
    let root = storage
        .stat_entry(ah_vfs::domain::storage::StorageStatEntryRequest {
            path: ah_vfs_storage::StoragePath::root(),
        })
        .await?;
    assert_eq!(root.entry_type(), ah_vfs_storage::EntryType::Directory);
    std::fs::remove_dir(&source)?;
    assert!(matches!(
        reopened.open(restored_registry.open_context(instance.id.clone()).await?).await,
        Err(DriverError::Lifecycle(error)) if error.failure() == LifecycleFailure::Backend && error.source().is_some()
    ));
    assert!(restored_registry.get(instance.id.clone()).await?.is_some());
    restored_registry.remove(instance.id.clone()).await?;
    assert!(matches!(
        restored_registry.open_context(instance.id.clone()).await,
        Err(DriverError::InstanceNotFound(_))
    ));
    pool.close().await;
    Ok(())
}

#[tokio::test]
async fn invalid_instance_rows_are_reported_without_silently_dropping_data() -> TestResult {
    for (id, json) in [
        ("bad id", "{}"),
        ("valid-id", "{}"),
        ("valid-id", "not-json"),
    ] {
        let pool = common::pool().await?;
        let driver = LinuxLocalDriver::new()?;
        let registry = common::registry(&driver, pool.clone()).await?;
        sqlx::query("INSERT INTO vfs_instance_records VALUES ('local', ?, 1, 1, ?, 0, 0)")
            .bind(id)
            .bind(json)
            .execute(&pool)
            .await?;
        assert!(
            matches!(registry.list(InstanceQuery::default()).await, Err(DriverError::Lifecycle(error)) if error.failure() == ah_vfs::application::driver_error::LifecycleFailure::InvalidRecord)
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vfs_instance_records")
                .fetch_one(&pool)
                .await?,
            1
        );
        pool.close().await;
    }
    Ok(())
}

#[tokio::test]
async fn missing_instance_and_invalid_sources_return_errors_without_persisting() -> TestResult {
    let dir = tempfile::tempdir()?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    let driver = LinuxLocalDriver::new()?;
    let registry = common::registry(&driver, pool.clone()).await?;
    let id = InstanceId::parse("missing")?;
    assert!(matches!(
        registry.open_context(id.clone()).await,
        Err(DriverError::InstanceNotFound(_))
    ));
    assert!(matches!(
        registry.remove(id).await,
        Ok(RemoveInstanceOutcome::AlreadyAbsent)
    ));
    let file = dir.path().join("file");
    std::fs::write(&file, "untouched")?;
    assert!(matches!(
        registry.create(common::local_config(&file)?).await,
        Err(DriverError::Lifecycle(error)) if error.failure() == LifecycleFailure::InvalidRecord && error.source().is_some()
    ));
    assert!(
        registry
            .create(common::local_config(&dir.path().join("missing"))?)
            .await
            .is_err()
    );
    assert!(
        registry
            .list(InstanceQuery::default())
            .await?
            .instances
            .is_empty()
    );
    pool.close().await;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_canonical_roots_are_rejected_without_lossy_storage() -> TestResult {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let dir = tempfile::tempdir()?;
    let source = dir.path().join(OsString::from_vec(b"source-\xff".to_vec()));
    std::fs::create_dir(&source)?;
    let alias = dir.path().join("utf8-alias");
    std::os::unix::fs::symlink(&source, &alias)?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    let driver = LinuxLocalDriver::new()?;
    let registry = common::registry(&driver, pool.clone()).await?;
    assert!(matches!(
        registry.create(common::local_config(&alias)?).await,
        Err(DriverError::Lifecycle(error)) if error.failure() == LifecycleFailure::InvalidRecord && error.source().is_some()
    ));
    assert!(
        registry
            .list(InstanceQuery::default())
            .await?
            .instances
            .is_empty()
    );
    assert!(source.is_dir());
    pool.close().await;
    Ok(())
}
