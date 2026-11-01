#![cfg(target_os = "windows")]
mod common;

use ah_vfs::{application::port::Driver, testing::assert_range_read_contract};
use vfs_driver_local_windows::WindowsLocalDriver;

#[tokio::test]
async fn local_range_reads_obey_storage_contract() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let driver = WindowsLocalDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let instance = registry.create(common::local_config(dir.path())?).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    assert_range_read_contract(storage.as_ref()).await
}
