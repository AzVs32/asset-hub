use ah_vfs::application::dto::{InstanceQuery, RemoveInstanceOutcome};
mod common;

use ah_vfs::{
    application::port::Driver,
    domain::{DriverError, InstanceId, StorageErrorKind},
};
use common::*;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::error::Error;
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

#[tokio::test]
async fn table_preserves_size_and_unit_and_only_owns_its_data() -> TestResult {
    let pool = pool().await?;
    sqlx::query("CREATE TABLE application_data(value TEXT)")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO application_data VALUES ('retained')")
        .execute(&pool)
        .await?;
    let driver = MemoryDriver::from_pool(pool.clone()).await?;
    let one = driver
        .create_instance(common::memory_config(2, CapacityUnit::MiB))
        .await?;
    let two = driver
        .create_instance(common::memory_config(3, CapacityUnit::KiB))
        .await?;
    let row = sqlx::query("SELECT capacity_size, capacity_unit FROM memory_instances WHERE id = ?")
        .bind(one.id.as_str())
        .fetch_one(&pool)
        .await?;
    assert_eq!(row.try_get::<i64, _>("capacity_size")?, 2);
    assert_eq!(row.try_get::<String, _>("capacity_unit")?, "MiB");
    assert_eq!(
        driver
            .get_instance(one.id.clone())
            .await?
            .ok_or("missing")?
            .details["capacity_size"]
            .as_u64(),
        Some(2)
    );
    let listed = driver
        .list_instances(InstanceQuery::default())
        .await?
        .instances;
    assert_eq!(listed.len(), 2);
    assert!(
        listed
            .windows(2)
            .all(|items| items[0].id.as_str() < items[1].id.as_str())
    );
    assert_ne!(one.id, two.id);
    MemoryDriver::from_pool(pool.clone()).await?;
    assert_eq!(
        driver
            .list_instances(InstanceQuery::default())
            .await?
            .instances
            .len(),
        2
    );
    let value: String = sqlx::query_scalar("SELECT value FROM application_data")
        .fetch_one(&pool)
        .await?;
    assert_eq!(value, "retained");
    Ok(())
}

#[tokio::test]
async fn repeated_opens_share_data_but_instances_are_isolated() -> TestResult {
    let driver = MemoryDriver::from_pool(pool().await?).await?;
    let one = driver
        .create_instance(common::memory_config(1, CapacityUnit::KiB))
        .await?;
    let two = driver
        .create_instance(common::memory_config(1, CapacityUnit::KiB))
        .await?;
    let first = driver.open(one.id.clone().into()).await?;
    put(first.as_ref(), "file", b"shared").await?;
    drop(first);
    assert_eq!(
        get(driver.open(one.id.clone().into()).await?.as_ref(), "file").await?,
        b"shared"
    );
    assert!(!exists(driver.open(two.id.clone().into()).await?.as_ref(), "file").await?);
    Ok(())
}

#[tokio::test]
async fn settings_survive_restart_while_file_contents_are_volatile() -> TestResult {
    let dir = tempfile::tempdir()?;
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("driver.sqlite"))
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await?;
    let driver = MemoryDriver::from_pool(pool.clone()).await?;
    let instance = driver
        .create_instance(common::memory_config(8, CapacityUnit::KiB))
        .await?;
    put(
        driver.open(instance.id.clone().into()).await?.as_ref(),
        "file",
        b"volatile",
    )
    .await?;
    drop(driver);
    pool.close().await;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let driver = MemoryDriver::from_pool(pool).await?;
    assert_eq!(
        driver
            .get_instance(instance.id.clone())
            .await?
            .ok_or("missing")?
            .details["capacity_size"]
            .as_u64(),
        Some(8)
    );
    assert!(
        !exists(
            driver.open(instance.id.clone().into()).await?.as_ref(),
            "file"
        )
        .await?
    );
    Ok(())
}

#[tokio::test]
async fn deletion_blocks_new_opens_but_retains_existing_handles() -> TestResult {
    let driver = MemoryDriver::from_pool(pool().await?).await?;
    let instance = driver
        .create_instance(common::memory_config(64, CapacityUnit::B))
        .await?;
    let storage = driver.open(instance.id.clone().into()).await?;
    put(storage.as_ref(), "file", b"retained").await?;
    driver.remove_instance(instance.id.clone()).await?;
    assert!(
        matches!(driver.open(instance.id.clone().into()).await, Err(DriverError::InstanceNotFound(id)) if id == instance.id)
    );
    assert!(matches!(
        driver.remove_instance(instance.id.clone()).await,
        Ok(RemoveInstanceOutcome::AlreadyAbsent)
    ));
    drop(driver);
    assert_eq!(get(storage.as_ref(), "file").await?, b"retained");
    put(storage.as_ref(), "another", b"live").await?;
    Ok(())
}

#[tokio::test]
async fn invalid_capacity_and_corrupt_rows_are_rejected_without_replacing_data() -> TestResult {
    let pool = pool().await?;
    let driver = MemoryDriver::from_pool(pool.clone()).await?;
    assert!(matches!(
        driver
            .create_instance(common::memory_config(0, CapacityUnit::MiB))
            .await,
        Err(DriverError::InvalidInstanceConfig { .. })
    ));
    assert!(
        driver
            .list_instances(InstanceQuery::default())
            .await?
            .instances
            .is_empty()
    );
    for (id, size, unit) in [("bad id", 1, "B"), ("overflow", i64::MAX, "TiB")] {
        sqlx::query("INSERT INTO memory_instances VALUES (?, ?, ?)")
            .bind(id)
            .bind(size)
            .bind(unit)
            .execute(&pool)
            .await?;
        assert!(matches!(
            driver.list_instances(InstanceQuery::default()).await,
            Err(DriverError::Storage(error)) if error.kind() == StorageErrorKind::InvalidData
        ));
        sqlx::query("DELETE FROM memory_instances WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await?;
    }
    for (size, unit) in [(0, "B"), (-1, "MiB"), (1, "KB")] {
        assert!(
            sqlx::query("INSERT INTO memory_instances VALUES ('invalid', ?, ?)")
                .bind(size)
                .bind(unit)
                .execute(&pool)
                .await
                .is_err()
        );
    }
    assert!(
        driver
            .list_instances(InstanceQuery::default())
            .await?
            .instances
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn open_reports_domain_errors_and_keeps_original_sources() -> TestResult {
    let pool = pool().await?;
    let driver = MemoryDriver::from_pool(pool.clone()).await?;
    assert!(matches!(
        driver.open(InstanceId::parse("missing")?.into()).await,
        Err(DriverError::InstanceNotFound(_))
    ));
    pool.close().await;
    let failure = driver
        .open(InstanceId::parse("instance")?.into())
        .await
        .err()
        .ok_or("expected failure")?;
    let DriverError::Storage(error) = failure else {
        return Err("wrong domain error".into());
    };
    assert_eq!(error.kind(), StorageErrorKind::Unavailable);
    assert!(error.source().is_some());
    Ok(())
}
