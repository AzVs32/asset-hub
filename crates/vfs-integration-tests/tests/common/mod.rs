#![allow(dead_code)]

use ah_vfs::{
    application::{
        dto::{StorageReadFileRequest, StorageWriteFileRequest},
        port::{Storage, StorageFileReader},
    },
    domain::StoragePath,
};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub struct LocalPlugin {
    pub driver: std::sync::Arc<vfs_plugin_host::PluginDriver>,
    task: tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
}
impl Drop for LocalPlugin {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl LocalPlugin {
    pub async fn start(pool: sqlx::SqlitePool) -> TestResult<Self> {
        let native = std::sync::Arc::new(vfs_driver_local::LocalDriver::from_pool(pool).await?);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let task = tokio::spawn(vfs_plugin_grpc_server::serve(
            listener,
            native,
            "isolated-mount-test",
            std::future::pending(),
        ));
        let transport =
            vfs_plugin_grpc::GrpcTransport::connect(endpoint, "isolated-mount-test").await?;
        let driver = std::sync::Arc::new(
            vfs_plugin_host::PluginDriver::connect(std::sync::Arc::new(transport)).await?,
        );
        Ok(Self { driver, task })
    }
}

pub fn local_config(
    root: &std::path::Path,
) -> TestResult<ah_vfs::application::dto::InstanceConfig> {
    Ok(ah_vfs::application::dto::InstanceConfig::from([(
        "root_path".into(),
        root.to_str().ok_or("UTF-8 root required")?.into(),
    )]))
}

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
