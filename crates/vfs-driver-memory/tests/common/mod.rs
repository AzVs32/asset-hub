#![allow(dead_code)]
use ah_vfs::resolve::StorageOperations;
use ah_vfs::resolve::path::RootedPath;
use ah_vfs_storage::{Failure, Rejection};

use ah_vfs::application::port::{Storage, StorageFileReader};
use ah_vfs_storage::StoragePath;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub fn memory_config(
    size: u64,
    unit: vfs_driver_memory::CapacityUnit,
) -> ah_vfs::application::dto::InstanceConfig {
    ah_vfs::application::dto::InstanceConfig::from([
        ("capacity_size".into(), size.into()),
        ("capacity_unit".into(), unit.as_str().into()),
    ])
}

pub fn path(value: &str) -> TestResult<StoragePath> {
    Ok(StoragePath::parse(value)?)
}

pub async fn pool() -> TestResult<sqlx::SqlitePool> {
    Ok(sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?)
}

pub async fn put(storage: &dyn Storage, name: &str, bytes: &[u8]) -> TestResult {
    let mut writer = storage
        .write_file(
            ah_vfs::resolve::operand::WriteOperand::new(
                path(name)?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(bytes).await?;
    writer.commit().await?;
    Ok(())
}

pub async fn collect(reader: &mut dyn StorageFileReader) -> TestResult<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        assert!(!chunk.is_empty());
        bytes.extend(chunk);
    }
    assert!(reader.next_chunk().await?.is_none());
    Ok(bytes)
}

pub async fn get(storage: &dyn Storage, name: &str) -> TestResult<Vec<u8>> {
    collect(
        storage
            .read_file(
                ah_vfs::resolve::operand::ReadOperand::new(
                    path(name)?.to_rooted_expr(),
                    ah_vfs_storage::ReadRange::FULL,
                ),
                Default::default(),
            )
            .await?
            .as_mut(),
    )
    .await
}

pub async fn mkdir(storage: &dyn Storage, name: &str) -> TestResult {
    storage
        .create_dir(
            ah_vfs::resolve::operand::CreateDirectoryOperand::new(
                path(name)?.to_rooted_expr(),
                ah_vfs::resolve::operand::CreateDirectoryMode::Recursive,
            ),
            Default::default(),
        )
        .await?;
    Ok(())
}

pub async fn exists(storage: &dyn Storage, name: &str) -> TestResult<bool> {
    match storage
        .stat_entry(
            ah_vfs::resolve::operand::StatOperand::new(
                path(name)?.to_rooted_expr(),
                ah_vfs::resolve::policy::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub async fn registry(
    driver: &dyn ah_vfs::application::port::Driver,
    pool: sqlx::SqlitePool,
) -> Result<
    ah_vfs::application::host_instance_registry::HostInstanceRegistry,
    Box<dyn std::error::Error>,
> {
    let provisioner = driver.provisioner();
    Ok(ah_vfs::application::host_instance_registry::HostInstanceRegistry::new(
        driver.info(), provisioner,
        std::sync::Arc::new(ah_vfs::infrastructure::instance_record_repository::SqliteInstanceRecordRepository::from_pool(pool).await?), None))
}
