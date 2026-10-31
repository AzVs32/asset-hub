#![cfg(target_os = "windows")]
mod common;

use ah_vfs::{application::port::Driver, testing::assert_pagination_contract};
use vfs_driver_local_windows::WindowsLocalDriver;

#[tokio::test]
async fn local_uses_shared_pagination_validation() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let driver = WindowsLocalDriver::from_pool(common::pool().await?).await?;
    let instance = driver
        .create_instance(common::local_config(dir.path())?)
        .await?;
    let storage = driver.open(instance.id.clone().into()).await?;
    assert_pagination_contract(storage.as_ref()).await
}
