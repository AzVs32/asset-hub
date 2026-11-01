#![allow(dead_code)]

use ah_vfs::{
    application::{
        dto::{
            StorageCreateDirRequest, StorageReadFileRequest, StorageStatEntryRequest,
            StorageWriteFileRequest,
        },
        port::{Storage, StorageFileReader},
    },
    domain::StoragePath,
};

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
        .write_file(StorageWriteFileRequest::new(path(name)?))
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
            .read_file(StorageReadFileRequest::new(path(name)?))
            .await?
            .as_mut(),
    )
    .await
}

pub async fn mkdir(storage: &dyn Storage, name: &str) -> TestResult {
    storage
        .create_dir(StorageCreateDirRequest {
            path: path(name)?,
            recursive: true,
        })
        .await?;
    Ok(())
}

pub async fn exists(storage: &dyn Storage, name: &str) -> TestResult<bool> {
    match storage
        .stat_entry(StorageStatEntryRequest { path: path(name)? })
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ah_vfs::domain::StorageErrorKind::NotFound => Ok(false),
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
