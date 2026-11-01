#![cfg(target_os = "linux")]
mod common;

use ah_vfs::{application::port::Driver, testing::assert_same_path_rename_contract};
use vfs_driver_local_linux::LinuxLocalDriver;

#[tokio::test]
async fn local_same_path_rename_obeys_storage_contract() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let driver = LinuxLocalDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let instance = registry.create(common::local_config(dir.path())?).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    assert_same_path_rename_contract(storage.as_ref()).await
}
