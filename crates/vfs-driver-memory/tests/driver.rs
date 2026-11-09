use ah_vfs::application::driver_error::LifecycleFailure;
mod common;
use ah_vfs::application::dto::{InstanceConfig, InstanceQuery, RemoveInstanceOutcome};
use ah_vfs::application::port::Driver;
use ah_vfs::domain::InstanceId;

use ah_vfs::application::driver_error::DriverError;
use common::*;
use std::sync::Arc;
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

#[tokio::test]
async fn host_records_preserve_details_and_leave_other_tables_untouched() -> TestResult {
    let pool = pool().await?;
    sqlx::query("CREATE TABLE application_data(value TEXT)")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO application_data VALUES ('retained')")
        .execute(&pool)
        .await?;
    let driver = MemoryDriver::new()?;
    let registry = registry(&driver, pool.clone()).await?;
    let one = registry.create(memory_config(2, CapacityUnit::MiB)).await?;
    let two = registry.create(memory_config(3, CapacityUnit::KiB)).await?;
    assert_eq!(one.details["capacity_size"], 2);
    assert_eq!(one.details["capacity_unit"], "MiB");
    assert_eq!(registry.get(one.id.clone()).await?, Some(one.clone()));
    let listed = registry.list(InstanceQuery::default()).await?.instances;
    assert_eq!(listed.len(), 2);
    assert!(
        listed
            .windows(2)
            .all(|pair| pair[0].id.as_str() < pair[1].id.as_str())
    );
    assert_ne!(one.id, two.id);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT value FROM application_data")
            .fetch_one(&pool)
            .await?,
        "retained"
    );
    Ok(())
}
#[tokio::test]
async fn concurrent_repeated_opens_share_contents_and_isolate_instances() -> TestResult {
    let driver = MemoryDriver::new()?;
    let registry = registry(&driver, pool().await?).await?;
    let one = registry.create(memory_config(1, CapacityUnit::KiB)).await?;
    let two = registry.create(memory_config(1, CapacityUnit::KiB)).await?;
    let (a, b) = tokio::join!(
        driver.open(registry.open_context(one.id.clone()).await?),
        driver.open(registry.open_context(one.id.clone()).await?)
    );
    let (a, b) = (a?, b?);
    assert!(Arc::ptr_eq(&a, &b));
    put(a.as_ref(), "file", b"shared").await?;
    drop(a);
    drop(b);
    assert_eq!(
        get(
            driver
                .open(registry.open_context(one.id).await?)
                .await?
                .as_ref(),
            "file"
        )
        .await?,
        b"shared"
    );
    assert!(
        !exists(
            driver
                .open(registry.open_context(two.id).await?)
                .await?
                .as_ref(),
            "file"
        )
        .await?
    );
    Ok(())
}
#[tokio::test]
async fn host_settings_survive_restart_but_memory_contents_do_not() -> TestResult {
    let dir = tempfile::tempdir()?;
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(dir.path().join("host.sqlite"))
        .create_if_missing(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await?;
    let driver = MemoryDriver::new()?;
    let host = registry(&driver, pool.clone()).await?;
    let instance = host.create(memory_config(8, CapacityUnit::KiB)).await?;
    put(
        driver
            .open(host.open_context(instance.id.clone()).await?)
            .await?
            .as_ref(),
        "file",
        b"volatile",
    )
    .await?;
    drop(driver);
    drop(host);
    pool.close().await;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let driver = MemoryDriver::new()?;
    let host = registry(&driver, pool).await?;
    assert_eq!(
        host.get(instance.id.clone())
            .await?
            .ok_or("missing")?
            .details["capacity_size"],
        8
    );
    assert!(
        !exists(
            driver
                .open(host.open_context(instance.id).await?)
                .await?
                .as_ref(),
            "file"
        )
        .await?
    );
    Ok(())
}
#[tokio::test]
async fn deletion_blocks_host_opens_and_retains_data_until_driver_is_dropped() -> TestResult {
    let driver = MemoryDriver::new()?;
    let registry = registry(&driver, pool().await?).await?;
    let instance = registry.create(memory_config(64, CapacityUnit::B)).await?;
    let saved_context = registry.open_context(instance.id.clone()).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    put(storage.as_ref(), "file", b"retained").await?;
    assert_eq!(
        registry.remove(instance.id.clone()).await?,
        RemoveInstanceOutcome::Removed
    );
    assert!(matches!(
        registry.open_context(instance.id.clone()).await,
        Err(DriverError::InstanceNotFound(_))
    ));
    assert_eq!(
        registry.remove(instance.id).await?,
        RemoveInstanceOutcome::AlreadyAbsent
    );
    let retained = driver.open(saved_context).await?;
    assert!(Arc::ptr_eq(&storage, &retained));
    drop(driver);
    assert_eq!(get(storage.as_ref(), "file").await?, b"retained");
    put(storage.as_ref(), "another", b"live").await?;
    Ok(())
}
#[tokio::test]
async fn prepare_rejects_missing_unknown_wrong_type_and_invalid_capacity() -> TestResult {
    let driver = MemoryDriver::new()?;
    let provisioner = driver.provisioner();
    for config in [
        InstanceConfig::default(),
        InstanceConfig::from([("unknown".into(), true.into())]),
        InstanceConfig::from([
            ("capacity_size".into(), "1".into()),
            ("capacity_unit".into(), "B".into()),
        ]),
        InstanceConfig::from([
            ("capacity_size".into(), 1.into()),
            ("capacity_unit".into(), false.into()),
        ]),
        memory_config(0, CapacityUnit::B),
        memory_config(u64::MAX, CapacityUnit::TiB),
    ] {
        assert!(matches!(
            provisioner.prepare(config).await,
            Err(DriverError::InvalidInstanceConfig { .. })
        ));
    }
    let config = memory_config(4, CapacityUnit::KiB);
    let prepared = provisioner.prepare(config.clone()).await?;
    assert_eq!(prepared.config(), &config);
    assert_eq!(prepared.details()["capacity_size"], 4);
    Ok(())
}
#[tokio::test]
async fn immutable_capacity_updates_and_missing_or_closed_host_records_are_rejected() -> TestResult
{
    let driver = MemoryDriver::new()?;
    let pool = pool().await?;
    let registry = registry(&driver, pool.clone()).await?;
    let instance = registry.create(memory_config(2, CapacityUnit::KiB)).await?;
    for config in [
        memory_config(3, CapacityUnit::KiB),
        memory_config(2, CapacityUnit::MiB),
    ] {
        assert!(
            matches!(registry.update(instance.id.clone(),config).await,Err(DriverError::Lifecycle(error)) if error.failure()==LifecycleFailure::InvalidRequest)
        );
    }
    assert!(matches!(
        registry.open_context(InstanceId::parse("missing")?).await,
        Err(DriverError::InstanceNotFound(_))
    ));
    pool.close().await;
    assert!(
        matches!(registry.open_context(instance.id).await,Err(DriverError::Lifecycle(error)) if error.failure()==LifecycleFailure::Unavailable && std::error::Error::source(&error).is_some())
    );
    Ok(())
}
