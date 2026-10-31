#![cfg(target_os = "linux")]
mod common;

use ah_vfs::application::dto::{InstanceQuery, RemoveInstanceOutcome};
use ah_vfs::application::port::Driver;
use ah_vfs::domain::{DriverError, InstanceId};
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
    let driver = LinuxLocalDriver::from_pool(pool.clone()).await?;
    assert_eq!(driver.info().key().as_str(), "local");
    let instance = driver
        .create_instance(common::local_config(&source)?)
        .await?;
    assert_eq!(
        instance.details["root_path"].as_str(),
        std::fs::canonicalize(&source)?.to_str()
    );
    drop(driver);

    let reopened = LinuxLocalDriver::from_pool(pool.clone()).await?;
    let restored = reopened
        .get_instance(instance.id.clone())
        .await?
        .ok_or("instance missing")?;
    assert_eq!(
        restored.details["root_path"].as_str(),
        instance.details["root_path"].as_str()
    );
    let storage = reopened.open(instance.id.clone().into()).await?;
    let root = storage
        .stat_entry(ah_vfs::application::dto::StorageStatEntryRequest {
            path: ah_vfs::domain::StoragePath::root(),
        })
        .await?;
    assert_eq!(root.kind, ah_vfs::application::dto::StorageEntryKind::Dir);
    std::fs::remove_dir(&source)?;
    assert!(matches!(
        reopened.open(instance.id.clone().into()).await,
        Err(DriverError::Storage(error)) if error.kind() == ah_vfs::domain::StorageErrorKind::NotFound
    ));
    assert!(reopened.get_instance(instance.id.clone()).await?.is_some());
    reopened.remove_instance(instance.id.clone()).await?;
    assert!(matches!(
        reopened.open(instance.id.clone().into()).await,
        Err(DriverError::InstanceNotFound(_))
    ));
    pool.close().await;
    Ok(())
}

#[tokio::test]
async fn invalid_instance_rows_are_reported_without_silently_dropping_data() -> TestResult {
    for (id, root) in [
        ("bad id", "/absolute"),
        ("valid-id", "relative"),
        ("valid-id", "/path/../outside"),
    ] {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let driver = LinuxLocalDriver::from_pool(pool.clone()).await?;
        sqlx::query("INSERT INTO local_instances VALUES (?, ?)")
            .bind(id)
            .bind(root)
            .execute(&pool)
            .await?;
        assert!(matches!(
            driver.list_instances(InstanceQuery::default()).await,
            Err(DriverError::Storage(error)) if error.kind() == ah_vfs::domain::StorageErrorKind::InvalidData
        ));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM local_instances")
            .fetch_one(&pool)
            .await?;
        assert_eq!(count, 1);
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
    let driver = LinuxLocalDriver::from_pool(pool.clone()).await?;
    let id = InstanceId::parse("missing")?;
    assert!(matches!(
        driver.open(id.clone().into()).await,
        Err(DriverError::InstanceNotFound(_))
    ));
    assert!(matches!(
        driver.remove_instance(id).await,
        Ok(RemoveInstanceOutcome::AlreadyAbsent)
    ));
    let file = dir.path().join("file");
    std::fs::write(&file, "untouched")?;
    assert!(matches!(
        driver.create_instance(common::local_config(&file)?).await,
        Err(DriverError::Storage(error)) if error.kind() == ah_vfs::domain::StorageErrorKind::NotDir
    ));
    assert!(
        driver
            .create_instance(common::local_config(&dir.path().join("missing"))?)
            .await
            .is_err()
    );
    assert!(
        driver
            .list_instances(InstanceQuery::default())
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
    let driver = LinuxLocalDriver::from_pool(pool.clone()).await?;
    assert!(matches!(
        driver.create_instance(common::local_config(&alias)?).await,
        Err(DriverError::Storage(error)) if error.kind() == ah_vfs::domain::StorageErrorKind::InvalidNameEncoding
    ));
    assert!(
        driver
            .list_instances(InstanceQuery::default())
            .await?
            .instances
            .is_empty()
    );
    assert!(source.is_dir());
    pool.close().await;
    Ok(())
}
