mod common;

use ah_vfs::{application::port::Driver, testing::assert_same_path_rename_contract};
use common::TestResult;
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

#[tokio::test]
async fn memory_same_path_rename_obeys_storage_contract() -> TestResult {
    let driver = MemoryDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let instance = registry
        .create(common::memory_config(1, CapacityUnit::KiB))
        .await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    assert_same_path_rename_contract(storage.as_ref(), driver.info().capabilities().rename()).await
}
