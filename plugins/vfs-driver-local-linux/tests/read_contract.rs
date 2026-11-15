#![cfg(target_os = "linux")]
mod common;

use ah_vfs::application::port::Driver;
use ah_vfs::testing::{assert_range_read_contract, assert_stat_binding_contract};
use vfs_driver_local_linux::LinuxLocalDriver;

#[tokio::test]
async fn local_range_reads_obey_storage_contract() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let driver = LinuxLocalDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let instance = registry.create(common::local_config(dir.path())?).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    assert_stat_binding_contract(storage.as_ref()).await?;
    assert_range_read_contract(storage.as_ref()).await
}
