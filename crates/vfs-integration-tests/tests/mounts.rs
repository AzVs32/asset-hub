mod common;

use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        dto::{MountRequest, ReadFileRequest, StorageWriteFileMode, WriteFileRequest},
        mount_runtime::MountRuntime,
        port::Driver,
        service::{EntryService, MountService},
    },
    domain::{DriverInstanceRef, DriverKey, InstanceId, VirtualPath},
    infrastructure::mount_repository::SqliteMountRepository,
};
use common::*;
use std::sync::Arc;
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

fn virtual_path(value: &str) -> TestResult<VirtualPath> {
    Ok(VirtualPath::parse(value)?)
}
fn reference(id: &InstanceId) -> TestResult<DriverInstanceRef> {
    Ok(DriverInstanceRef::new(
        DriverKey::parse(MemoryDriver::KEY)?,
        id.clone(),
    ))
}

async fn write(entries: &EntryService, value: &str, bytes: &[u8]) -> TestResult {
    let mut writer = entries
        .write_file(WriteFileRequest {
            path: virtual_path(value)?,
            mode: StorageWriteFileMode::CreateNew,
            require_atomic_publish: true,
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
            offset: 0,
            length: None,
        })
        .await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        bytes.extend(chunk);
    }
    Ok(bytes)
}

#[tokio::test]
async fn local_plugin_and_builtin_memory_share_the_same_services_and_traits() -> TestResult {
    let pool = pool().await?;
    let dir = tempfile::tempdir()?;
    let plugin = LocalPlugin::start(pool.clone()).await?;
    let local = plugin.driver.clone();
    let memory = Arc::new(MemoryDriver::from_pool(pool.clone()).await?);
    let memory_instance = memory
        .create_instance(common::memory_config(4, CapacityUnit::KiB))
        .await?;
    let mut catalog = DriverCatalog::new();
    catalog.register(local.clone(), Default::default()).await?;
    catalog.register(memory.clone(), Default::default()).await?;
    let local_instance = local
        .instances()
        .registry()?
        .create(common::local_config(dir.path())?)
        .await?;
    let catalog = Arc::new(catalog);
    let repository = Arc::new(SqliteMountRepository::from_pool(pool.clone()).await?);
    let runtime = Arc::new(MountRuntime::new(repository, catalog));
    let mounts = MountService::new(runtime.clone());
    let entries = EntryService::new(runtime);
    mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/")?,
            driver_instance: DriverInstanceRef::new(
                DriverKey::parse("local")?,
                local_instance.id.clone(),
            ),
        })
        .await?;
    mounts
        .mount(MountRequest {
            virtual_path: virtual_path("/ram")?,
            driver_instance: reference(&memory_instance.id)?,
        })
        .await?;
    write(&entries, "/disk-file", b"on disk").await?;
    write(&entries, "/ram/memory-file", b"in memory").await?;
    assert_eq!(std::fs::read(dir.path().join("disk-file"))?, b"on disk");
    assert!(!dir.path().join("ram").exists());
    assert_eq!(read(&entries, "/ram/memory-file").await?, b"in memory");
    assert_eq!(
        get(
            memory
                .open(memory_instance.id.clone().into())
                .await?
                .as_ref(),
            "memory-file"
        )
        .await?,
        b"in memory"
    );
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(&pool)
            .await?;
    for table in ["memory_instances", "local_instances", "vfs_mounts"] {
        assert!(tables.iter().any(|name| name == table));
    }
    Ok(())
}
