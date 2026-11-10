use ah_vfs::application::mount_runtime::MountRuntime;
use ah_vfs_storage::{BackendFailure, Failure, Rejection};
mod common;

use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::dto::{
    CreateDirRequest, DirQuery, EntryOrigin, MountFilter, MountRequest, MoveEntryRequest,
    ReadFileRequest, RemoveEntryRequest, WriteFileRequest,
};
use ah_vfs::application::error::EntryError;
use ah_vfs::application::port::MountRepository;
use ah_vfs::application::service::{EntryService, MountService};
use ah_vfs::domain::namespace::{MountTableError, VirtualPath};
use ah_vfs::domain::{DriverInstance, DriverKey, InstanceId};
use ah_vfs::infrastructure::mount_repository::SqliteMountRepository;
use ah_vfs_storage::EntryType;
use ah_vfs_storage::WriteMode;
use common::*;
use std::{num::NonZeroUsize, sync::Arc};
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

struct Fixture {
    registry: ah_vfs::application::host_instance_registry::HostInstanceRegistry,
    root: ah_vfs::application::dto::InstanceInfo,
    repository: Arc<SqliteMountRepository>,
    mounts: MountService,
    entries: EntryService,
}

fn virtual_path(value: &str) -> TestResult<VirtualPath> {
    Ok(VirtualPath::parse(value)?)
}
fn reference(id: &InstanceId) -> TestResult<DriverInstance> {
    Ok(DriverInstance::new(
        DriverKey::parse(MemoryDriver::KEY)?,
        id.clone(),
    ))
}

async fn fixture() -> TestResult<Fixture> {
    let pool = pool().await?;
    let driver = Arc::new(MemoryDriver::new()?);
    let registry = common::registry(driver.as_ref(), pool.clone()).await?;
    let records = Arc::new(ah_vfs::infrastructure::instance_record_repository::SqliteInstanceRecordRepository::from_pool(pool.clone()).await?);
    let root = registry
        .create(common::memory_config(64, CapacityUnit::KiB))
        .await?;
    let repository = Arc::new(SqliteMountRepository::from_pool(pool).await?);
    let mut catalog = DriverCatalog::new();
    ah_vfs::testing::register_driver(&mut catalog, driver.clone())?;
    let catalog = Arc::new(catalog);
    let runtime = Arc::new(MountRuntime::new(
        repository.clone(),
        catalog,
        records,
        None,
    ));
    let mounts = MountService::new(runtime.clone());
    let entries = EntryService::new(runtime);
    mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/")?,
            driver_instance: reference(&root.id)?,
        })
        .await?;
    Ok(Fixture {
        registry,
        root,
        repository,
        mounts,
        entries,
    })
}

async fn write(entries: &EntryService, value: &str, bytes: &[u8]) -> TestResult {
    let mut writer = entries
        .write_file(WriteFileRequest {
            path: virtual_path(value)?,
            mode: WriteMode::CreateNew,
            publication: ah_vfs_storage::Publication::Atomic,
        })
        .await?;
    writer.write_chunk(bytes).await?;
    let entry = writer.commit().await?;
    assert_eq!(entry.path, virtual_path(value)?);
    Ok(())
}

async fn read(entries: &EntryService, value: &str) -> TestResult<Vec<u8>> {
    let mut reader = entries
        .read_file(ReadFileRequest {
            path: virtual_path(value)?,
            range: ah_vfs_storage::ReadRange::new(0, None)?,
        })
        .await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        bytes.extend(chunk);
    }
    Ok(bytes)
}

#[tokio::test]
async fn mount_disable_enable_unmount_remount_preserves_instance_and_open_streams() -> TestResult {
    let f = fixture().await?;
    let instance = f
        .registry
        .create(common::memory_config(8, CapacityUnit::KiB))
        .await?;
    let mounted = f
        .mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/ram")?,
            driver_instance: reference(&instance.id)?,
        })
        .await?;
    write(&f.entries, "/ram/file", b"data").await?;
    let mut writer = f
        .entries
        .write_file(WriteFileRequest {
            path: virtual_path("/ram/pending")?,
            mode: WriteMode::CreateNew,
            publication: ah_vfs_storage::Publication::Atomic,
        })
        .await?;
    writer.write_chunk(b"pending").await?;
    let mut reader = f
        .entries
        .read_file(ReadFileRequest {
            path: virtual_path("/ram/file")?,
            range: ah_vfs_storage::ReadRange::new(0, None)?,
        })
        .await?;
    f.mounts.disable(mounted.id).await?;
    assert!(matches!(
        f.entries
            .read_file(ReadFileRequest {
                path: virtual_path("/ram/file")?,
                range: ah_vfs_storage::ReadRange::new(0, None)?,
            })
            .await,
        Err(EntryError::Table(MountTableError::Disabled(_)))
    ));
    assert_eq!(reader.next_chunk().await?.ok_or("chunk")?, b"data");
    f.mounts.enable(mounted.id).await?;
    assert_eq!(read(&f.entries, "/ram/file").await?, b"data");
    f.mounts.unmount(mounted.id).await?;
    assert!(f.registry.get(instance.id.clone()).await?.is_some());
    writer.commit().await?;
    f.mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/ram")?,
            driver_instance: reference(&instance.id)?,
        })
        .await?;
    assert_eq!(read(&f.entries, "/ram/pending").await?, b"pending");
    let root_id = f
        .mounts
        .list_mounts(MountFilter::default())
        .await?
        .into_iter()
        .find(|mount| mount.virtual_path.is_root())
        .ok_or("root")?
        .id;
    assert!(matches!(
        f.mounts.unmount(root_id).await,
        Err(ah_vfs::application::error::MountError::Table(
            MountTableError::RootCannotBeRemoved
        ))
    ));
    Ok(())
}

#[tokio::test]
async fn missing_instances_and_duplicate_paths_do_not_persist_failed_mounts() -> TestResult {
    let f = fixture().await?;
    let failure = f
        .mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/missing")?,
            driver_instance: reference(&InstanceId::parse("missing")?)?,
        })
        .await;
    assert!(matches!(
        failure,
        Err(ah_vfs::application::error::MountError::Driver(
            DriverError::InstanceNotFound(_)
        ))
    ));
    assert_eq!(f.repository.list(MountFilter::default()).await?.len(), 1);
    f.mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/ram")?,
            driver_instance: reference(&f.root.id)?,
        })
        .await?;
    assert!(
        f.mounts
            .mount(MountRequest {
                virtual_path: virtual_path("//ram/")?,
                driver_instance: reference(&f.root.id)?
            })
            .await
            .is_err()
    );
    assert_eq!(f.repository.list(MountFilter::default()).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn nested_mounts_overlay_backend_entries_and_synthesize_missing_parents() -> TestResult {
    let f = fixture().await?;
    write(&f.entries, "/virtual", b"shadowed backend file").await?;
    let instance = f
        .registry
        .create(common::memory_config(1, CapacityUnit::KiB))
        .await?;
    let nested = f
        .mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/virtual/ram")?,
            driver_instance: reference(&instance.id)?,
        })
        .await?;
    write(&f.entries, "/virtual/ram/file", b"nested").await?;
    let view = f.entries.stat_entry(&virtual_path("/virtual")?).await?;
    assert_eq!(view.origin, EntryOrigin::SyntheticDir);
    assert_eq!(view.kind, EntryType::Directory);
    let page = f
        .entries
        .list_entries(&virtual_path("/virtual")?, DirQuery::default())
        .await?;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].path, virtual_path("/virtual/ram")?);
    assert_eq!(read(&f.entries, "/virtual/ram/file").await?, b"nested");
    f.mounts.disable(nested.id).await?;
    assert!(
        f.entries
            .read_file(ReadFileRequest {
                path: virtual_path("/virtual/ram/file")?,
                range: ah_vfs_storage::ReadRange::new(0, None)?,
            })
            .await
            .is_err()
    );
    f.mounts.unmount(nested.id).await?;
    assert_eq!(
        read(&f.entries, "/virtual").await?,
        b"shadowed backend file"
    );
    Ok(())
}

#[tokio::test]
async fn same_instance_can_be_mounted_twice_but_moves_cannot_cross_mount_boundaries() -> TestResult
{
    let f = fixture().await?;
    f.mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/alias")?,
            driver_instance: reference(&f.root.id)?,
        })
        .await?;
    write(&f.entries, "/file", b"shared").await?;
    assert_eq!(read(&f.entries, "/alias/file").await?, b"shared");
    assert!(matches!(
        f.entries
            .move_entry(MoveEntryRequest {
                source: virtual_path("/file")?,
                destination: virtual_path("/alias/new")?,
                replace: false
            })
            .await,
        Err(EntryError::CrossMountMove)
    ));
    assert!(matches!(
        f.entries
            .remove_entry(RemoveEntryRequest {
                path: virtual_path("/alias")?
            })
            .await,
        Err(EntryError::MountBoundary(_))
    ));
    Ok(())
}

#[tokio::test]
async fn create_dir_preserves_vfs_mount_boundaries_in_both_modes() -> TestResult {
    let f = fixture().await?;
    f.mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/gap/ram")?,
            driver_instance: reference(&f.root.id)?,
        })
        .await?;
    for name in ["/", "/gap", "/gap/ram"] {
        for recursive in [false, true] {
            let error = f
                .entries
                .create_dir(CreateDirRequest::new(
                    name,
                    if recursive {
                        ah_vfs_storage::CreateDirectoryMode::Recursive
                    } else {
                        ah_vfs_storage::CreateDirectoryMode::Single
                    },
                )?)
                .await
                .err()
                .ok_or("expected protected namespace entry")?;
            assert!(
                matches!(error, EntryError::MountBoundary(path) if path == virtual_path(name)?)
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn create_dir_at_the_application_entry_preserves_a_trailing_slash() -> TestResult {
    use ah_vfs_storage::CreateDirectoryMode::{Recursive, Single};

    let f = fixture().await?;
    let mut writer = f
        .entries
        .write_file(WriteFileRequest {
            path: virtual_path("/file")?,
            mode: WriteMode::CreateNew,
            publication: ah_vfs_storage::Publication::Atomic,
        })
        .await?;
    writer.commit().await?;
    for (name, rejection) in [
        ("/file", Rejection::AlreadyExists),
        ("/file/", Rejection::NotDirectory),
    ] {
        assert!(
            matches!(f.entries.create_dir(CreateDirRequest::new(name, Single)?).await,
            Err(error) if error.storage_error().is_some_and(|error| error.failure() == Failure::Rejected(rejection)))
        );
    }
    f.entries
        .create_dir(CreateDirRequest::new("/parent/child/", Recursive)?)
        .await?;
    Ok(())
}

#[tokio::test]
async fn capacity_and_unsupported_attribute_failures_reach_vfs_as_domain_errors() -> TestResult {
    let f = fixture().await?;
    let instance = f
        .registry
        .create(common::memory_config(3, CapacityUnit::B))
        .await?;
    f.mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/small")?,
            driver_instance: reference(&instance.id)?,
        })
        .await?;
    let mut writer = f
        .entries
        .write_file(WriteFileRequest {
            path: virtual_path("/small/file")?,
            mode: WriteMode::CreateNew,
            publication: ah_vfs_storage::Publication::Atomic,
        })
        .await?;
    let error = writer
        .write_chunk(b"full")
        .await
        .err()
        .ok_or("quota error")?;
    assert_eq!(
        error.storage_error().ok_or("domain")?.failure(),
        Failure::Backend(BackendFailure::CapacityExceeded)
    );
    assert!(
        matches!(writer.abort().await, Err(error) if error.storage_error().is_some_and(|error| error.failure() == Failure::Backend(BackendFailure::Closed)))
    );
    let error = f
        .entries
        .get_permissions(&virtual_path("/small")?)
        .await
        .err()
        .ok_or("unsupported")?;
    assert_eq!(
        error.storage_error().ok_or("domain")?.failure(),
        Failure::Rejected(Rejection::Unsupported)
    );
    let error = f
        .entries
        .list_xattrs(&virtual_path("/small")?, NonZeroUsize::MIN)
        .await
        .err()
        .ok_or("unsupported")?;
    assert_eq!(
        error.storage_error().ok_or("domain")?.failure(),
        Failure::Rejected(Rejection::Unsupported)
    );
    Ok(())
}

#[tokio::test]
async fn root_initialization_and_enable_persist_configuration_without_opening_backend() -> TestResult
{
    let driver = Arc::new(MemoryDriver::new()?);
    let repository = Arc::new(SqliteMountRepository::in_memory().await?);
    let mut catalog = DriverCatalog::new();
    ah_vfs::testing::register_driver(&mut catalog, driver)?;
    let catalog = Arc::new(catalog);
    let runtime = Arc::new(MountRuntime::new(repository, catalog, Arc::new(ah_vfs::infrastructure::instance_record_repository::SqliteInstanceRecordRepository::from_pool(pool().await?).await?), None));
    let mounts = MountService::new(runtime.clone());
    let entries = EntryService::new(runtime);
    let id = mounts
        .initialize_root(reference(&InstanceId::parse("missing")?)?)
        .await?;
    mounts.disable(id).await?;
    mounts.enable(id).await?;
    assert!(entries.capabilities(&virtual_path("/")?).await?.stat());
    let error = entries
        .stat_entry(&virtual_path("/file")?)
        .await
        .err()
        .ok_or("opening must fail")?;
    assert!(matches!(
        error,
        EntryError::Driver(DriverError::InstanceNotFound(_))
    ));
    Ok(())
}
