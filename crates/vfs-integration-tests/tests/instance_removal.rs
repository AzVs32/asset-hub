use ah_vfs::application::mount_runtime::MountRuntime;
mod common;

use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        dto::{
            InstanceQuery, MountFilter, MountRequest, RemoveInstanceOutcome,
            StorageReadFileRequest, StorageWriteFileRequest,
        },
        error::InstanceError,
        port::Driver,
        service::{InstanceService, MountService},
    },
    domain::{DriverError, DriverInstanceRef, StorageErrorKind, VirtualPath},
    infrastructure::mount_repository::SqliteMountRepository,
};
use common::*;
use std::{error::Error, sync::Arc};
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

struct Fixture {
    driver: Arc<dyn Driver>,
    pool: sqlx::SqlitePool,
    target: DriverInstanceRef,
    other: DriverInstanceRef,
    instances: InstanceService,
    mounts: MountService,
    root: tempfile::TempDir,
    _other_root: tempfile::TempDir,
}

impl Fixture {
    async fn new(memory: bool) -> TestResult<Self> {
        let pool = pool().await?;
        let root = tempfile::tempdir()?;
        let other_root = tempfile::tempdir()?;
        let (driver, target, other): (Arc<dyn Driver>, _, _) = if memory {
            let driver = Arc::new(MemoryDriver::from_pool(pool.clone()).await?);
            let target = driver
                .create_instance(common::memory_config(1, CapacityUnit::MiB))
                .await?
                .id
                .clone();
            let other = driver
                .create_instance(common::memory_config(1, CapacityUnit::MiB))
                .await?
                .id
                .clone();
            (driver, target, other)
        } else {
            let driver = Arc::new(vfs_driver_local::LocalDriver::from_pool(pool.clone()).await?);
            let target = driver
                .create_instance(common::local_config(root.path())?)
                .await?
                .id
                .clone();
            let other = driver
                .create_instance(common::local_config(other_root.path())?)
                .await?
                .id
                .clone();
            (driver, target, other)
        };
        let target = DriverInstanceRef::new(driver.info().key().clone(), target);
        let other = DriverInstanceRef::new(driver.info().key().clone(), other);
        let repository = Arc::new(SqliteMountRepository::in_memory().await?);
        let mut catalog = DriverCatalog::new();
        ah_vfs::testing::register_driver(&mut catalog, driver.clone())?;
        let catalog = Arc::new(catalog);
        let runtime = Arc::new(MountRuntime::new(repository, catalog));
        let instances = InstanceService::new(runtime.clone());
        let mounts = MountService::new(runtime);
        mounts
            .mount(MountRequest {
                virtual_path: VirtualPath::root(),
                driver_instance: other.clone(),
            })
            .await?;
        Ok(Self {
            driver,
            pool,
            target,
            other,
            instances,
            mounts,
            root,
            _other_root: other_root,
        })
    }
}

#[tokio::test]
async fn removal_is_idempotent_and_only_affects_the_requested_registration() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        assert_eq!(
            f.instances.remove_instance(&f.target).await?,
            RemoveInstanceOutcome::Removed
        );
        assert!(f.instances.get_instance(&f.target).await?.is_none());
        let page = f
            .instances
            .list_instances(f.target.driver_key(), InstanceQuery::default())
            .await?;
        assert_eq!(page.instances.len(), 1);
        assert_eq!(&page.instances[0].id, f.other.instance_id());
        assert!(matches!(
            f.driver.open(f.target.instance_id().clone().into()).await,
            Err(DriverError::InstanceNotFound(_))
        ));
        assert_eq!(
            f.instances.remove_instance(&f.target).await?,
            RemoveInstanceOutcome::AlreadyAbsent
        );
        assert!(f.instances.get_instance(&f.other).await?.is_some());
        assert_eq!(f.mounts.list_mounts(MountFilter::default()).await?.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn existing_sessions_outlive_removal_and_driver_retention_is_released() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        let storage = f.driver.open(f.target.instance_id().clone().into()).await?;
        put(storage.as_ref(), "existing", b"retained").await?;
        let mut reader = storage
            .read_file(StorageReadFileRequest::new(path("existing")?))
            .await?;
        let mut writer = storage
            .write_file(StorageWriteFileRequest::new(path("pending")?))
            .await?;
        writer.write_chunk(b"before-").await?;
        let weak = Arc::downgrade(&storage);
        drop(storage);
        assert_eq!(
            f.instances.remove_instance(&f.target).await?,
            RemoveInstanceOutcome::Removed
        );
        assert!(weak.upgrade().is_none());
        assert_eq!(collect(reader.as_mut()).await?, b"retained");
        writer.write_chunk(b"after").await?;
        assert_eq!(writer.commit().await?.size, Some(12));
        if !memory {
            assert_eq!(std::fs::read(f.root.path().join("existing"))?, b"retained");
            assert_eq!(
                std::fs::read(f.root.path().join("pending"))?,
                b"before-after"
            );
        }
        assert!(matches!(
            f.driver.open(f.target.instance_id().clone().into()).await,
            Err(DriverError::InstanceNotFound(_))
        ));
    }
    Ok(())
}

#[tokio::test]
async fn disabled_aliases_must_be_unmounted_before_removal() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        let first = f
            .mounts
            .mount(MountRequest {
                virtual_path: VirtualPath::parse("/first")?,
                driver_instance: f.target.clone(),
            })
            .await?;
        let second = f
            .mounts
            .mount(MountRequest {
                virtual_path: VirtualPath::parse("/second")?,
                driver_instance: f.target.clone(),
            })
            .await?;
        f.mounts.disable(first.id).await?;
        let error = f
            .instances
            .remove_instance(&f.target)
            .await
            .err()
            .ok_or("referenced")?;
        assert!(
            matches!(error, InstanceError::InstanceInUse { instance, mounts } if instance == f.target && mounts == vec![first.id, second.id])
        );
        assert!(f.instances.get_instance(&f.target).await?.is_some());
        f.mounts.unmount(first.id).await?;
        assert!(matches!(
            f.instances.remove_instance(&f.target).await,
            Err(InstanceError::InstanceInUse { .. })
        ));
        f.mounts.unmount(second.id).await?;
        assert_eq!(
            f.instances.remove_instance(&f.target).await?,
            RemoveInstanceOutcome::Removed
        );
    }
    Ok(())
}

#[tokio::test]
async fn root_registration_is_protected_by_its_mount_reference() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        assert!(matches!(
            f.instances.remove_instance(&f.other).await,
            Err(InstanceError::InstanceInUse { .. })
        ));
        assert!(f.instances.get_instance(&f.other).await?.is_some());
    }
    Ok(())
}

#[tokio::test]
async fn unreferenced_unavailable_or_corrupt_instances_can_be_removed_without_opening() -> TestResult
{
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        if memory {
            sqlx::query(
                "UPDATE memory_instances SET capacity_size = ?, capacity_unit = 'TiB' WHERE id = ?",
            )
            .bind(i64::MAX)
            .bind(f.target.instance_id().as_str())
            .execute(&f.pool)
            .await?;
            assert!(f.instances.get_instance(&f.target).await.is_err());
        } else {
            std::fs::remove_dir(f.root.path())?;
            assert!(
                f.driver
                    .open(f.target.instance_id().clone().into())
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            f.instances.remove_instance(&f.target).await?,
            RemoveInstanceOutcome::Removed
        );
        assert!(f.instances.get_instance(&f.target).await?.is_none());
        assert!(f.instances.get_instance(&f.other).await?.is_some());
    }
    Ok(())
}

#[tokio::test]
async fn failed_persistence_is_not_reported_as_removed_or_already_absent() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        let storage = f.driver.open(f.target.instance_id().clone().into()).await?;
        put(storage.as_ref(), "existing", b"retained").await?;
        f.pool.close().await;
        let error = f
            .instances
            .remove_instance(&f.target)
            .await
            .err()
            .ok_or("persistence failure")?;
        let backend = error.storage_error().ok_or("domain storage failure")?;
        assert_eq!(backend.kind(), StorageErrorKind::Unavailable);
        assert!(backend.source().is_some());
        assert_eq!(get(storage.as_ref(), "existing").await?, b"retained");
    }
    Ok(())
}

#[tokio::test]
async fn already_absent_memory_records_still_release_stale_runtime_retention() -> TestResult {
    let f = Fixture::new(true).await?;
    let storage = f.driver.open(f.target.instance_id().clone().into()).await?;
    put(storage.as_ref(), "existing", b"retained").await?;
    let mut reader = storage
        .read_file(StorageReadFileRequest::new(path("existing")?))
        .await?;
    let weak = Arc::downgrade(&storage);
    drop(storage);
    assert!(weak.upgrade().is_some());
    sqlx::query("DELETE FROM memory_instances WHERE id = ?")
        .bind(f.target.instance_id().as_str())
        .execute(&f.pool)
        .await?;
    assert_eq!(
        f.instances.remove_instance(&f.target).await?,
        RemoveInstanceOutcome::AlreadyAbsent
    );
    assert!(weak.upgrade().is_none());
    assert_eq!(collect(reader.as_mut()).await?, b"retained");
    Ok(())
}
