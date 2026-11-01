#![cfg(target_os = "linux")]
use ah_vfs::application::port::InstanceRegistry;
mod common;

use ah_vfs::{application::port::Driver, testing::assert_pagination_contract};
use vfs_driver_local_linux::LinuxLocalDriver;

#[tokio::test]
async fn local_uses_shared_pagination_validation() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let driver = LinuxLocalDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let instance = registry.create(common::local_config(dir.path())?).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    assert_pagination_contract(storage.as_ref()).await
}
