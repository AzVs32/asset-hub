#![cfg(target_os = "windows")]
//! End-to-end checks use the actual native plugin executable and a TCP channel.
use ah_vfs::application::driver_error::LifecycleFailure;
use ah_vfs_storage::{BackendFailure, Failure, MutationOutcome, Operation};
mod common;
use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::dto::*;
use ah_vfs::application::port::*;
use ah_vfs_storage::StoragePath;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::Arc,
};
use vfs_plugin_grpc::GrpcTransport;
use vfs_plugin_host::PluginDriver;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Process {
    child: Child,
    endpoint: String,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Process {
    fn start(address: &str) -> Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_vfs-driver-local-windows"))
            .arg(address)
            .env("VFS_PLUGIN_TOKEN", "isolated-test-token")
            .stdout(Stdio::piped())
            .spawn()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take().ok_or("stdout")?).read_line(&mut line)?;
        if line.trim().is_empty() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("plugin failed to start".into());
        }
        Ok(Self {
            child,
            endpoint: format!("http://{}", line.trim()),
        })
    }
    async fn driver(&self) -> Result<Arc<PluginDriver>> {
        Ok(Arc::new(
            PluginDriver::connect(Arc::new(
                GrpcTransport::connect(self.endpoint.clone(), "isolated-test-token").await?,
            ))
            .await?,
        ))
    }
}
async fn storage(
    driver: &dyn Driver,
    root: &std::path::Path,
    records: sqlx::SqlitePool,
) -> Result<Arc<dyn Storage>> {
    let registry = common::registry(driver, records).await?;
    let instance = registry.create(common::local_config(root)?).await?;
    Ok(driver
        .open(registry.open_context(instance.id).await?)
        .await?)
}
#[tokio::test]
async fn native_process_passes_shared_storage_contracts_over_grpc() -> Result {
    let records = common::pool().await?;
    let process = Process::start("127.0.0.1:0")?;
    let driver = process.driver().await?;
    let mut catalog = DriverCatalog::new();
    catalog.register(driver.clone(), Default::default()).await?;
    let root = tempfile::tempdir()?;
    ah_vfs::testing::assert_range_read_contract(
        storage(driver.as_ref(), root.path(), records.clone())
            .await?
            .as_ref(),
    )
    .await?;
    let root = tempfile::tempdir()?;
    ah_vfs::testing::assert_create_dir_contract(
        storage(driver.as_ref(), root.path(), records.clone())
            .await?
            .as_ref(),
    )
    .await?;
    let root = tempfile::tempdir()?;
    ah_vfs::testing::assert_pagination_contract(
        storage(driver.as_ref(), root.path(), records.clone())
            .await?
            .as_ref(),
    )
    .await?;
    let root = tempfile::tempdir()?;
    ah_vfs::testing::assert_same_path_rename_contract(
        storage(driver.as_ref(), root.path(), records.clone())
            .await?
            .as_ref(),
        driver.info().capabilities().rename(),
    )
    .await?;
    let root = tempfile::tempdir()?;
    let storage = storage(driver.as_ref(), root.path(), records.clone()).await?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                (StoragePath::parse("retained")?).to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"independent session").await?;
    let key = driver.info().key().clone();
    catalog.unregister(&key)?;
    drop(driver);
    drop(catalog);
    writer.commit().await?;
    let mut reader = storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                StoragePath::parse("retained")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    drop(storage);
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        bytes.extend(chunk);
    }
    assert_eq!(bytes, b"independent session");
    assert!(reader.next_chunk().await?.is_none());
    let denied = PluginDriver::connect(Arc::new(
        GrpcTransport::connect(process.endpoint.clone(), "wrong-token").await?,
    ))
    .await
    .err()
    .ok_or("authentication must fail")?;
    assert_eq!(
        denied.lifecycle_error().ok_or("category")?.failure(),
        LifecycleFailure::AccessDenied
    );
    Ok(())
}
#[tokio::test]
async fn process_restart_rejects_old_resources_without_replaying_commit() -> Result {
    let records = common::pool().await?;
    let mut process = Process::start("127.0.0.1:0")?;
    let address = process
        .endpoint
        .strip_prefix("http://")
        .ok_or("address")?
        .to_owned();
    let driver = process.driver().await?;
    let mut catalog = DriverCatalog::new();
    catalog.register(driver.clone(), Default::default()).await?;
    let root = tempfile::tempdir()?;
    let storage = storage(driver.as_ref(), root.path(), records.clone()).await?;
    let mut pending = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                (StoragePath::parse("uncertain")?).to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    pending.write_chunk(b"unpublished").await?;
    process.child.kill()?;
    process.child.wait()?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !storage.is_stale() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await?;
    let error = pending.commit().await.err().ok_or("commit must fail")?;
    assert_eq!(error.failure(), Failure::Backend(BackendFailure::Stale));
    assert_eq!(error.operation(), Operation::Write);
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    assert_eq!(
        pending.commit().await.err().ok_or("terminal")?.failure(),
        Failure::Backend(BackendFailure::Closed)
    );
    assert!(!root.path().join("uncertain").exists());
    let restarted = Process::start(&address)?;
    let fresh = restarted.driver().await?;
    let mut fresh_catalog = DriverCatalog::new();
    fresh_catalog
        .register(fresh.clone(), Default::default())
        .await?;
    let old = storage
        .stat_entry(
            ah_vfs_storage::StatRequest::new(
                (StoragePath::root()).to_rooted_expr(),
                ah_vfs_storage::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await
        .err()
        .ok_or("old generation must fail")?;
    assert_eq!(old.failure(), Failure::Backend(BackendFailure::Stale));
    assert_eq!(
        common::registry(fresh.as_ref(), records.clone())
            .await?
            .list(InstanceQuery::default())
            .await?
            .instances
            .len(),
        1
    );
    Ok(())
}
