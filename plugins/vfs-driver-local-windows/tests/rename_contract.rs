#![cfg(target_os = "windows")]
mod common;

use ah_vfs::{application::port::Driver, testing::assert_same_path_rename_contract};
use vfs_driver_local_windows::WindowsLocalDriver;

#[tokio::test]
async fn local_same_path_rename_obeys_storage_contract() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let driver = WindowsLocalDriver::from_pool(common::pool().await?).await?;
    let instance = driver
        .create_instance(common::local_config(dir.path())?)
        .await?;
    let storage = driver.open(instance.id.clone().into()).await?;
    assert_same_path_rename_contract(storage.as_ref()).await
}
