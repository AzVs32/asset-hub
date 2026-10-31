mod common;

use ah_vfs::{application::port::Driver, testing::assert_same_path_rename_contract};
use common::{TestResult, pool};
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

#[tokio::test]
async fn memory_same_path_rename_obeys_storage_contract() -> TestResult {
    let driver = MemoryDriver::from_pool(pool().await?).await?;
    let instance = driver
        .create_instance(common::memory_config(1, CapacityUnit::KiB))
        .await?;
    let storage = driver.open(instance.id.clone().into()).await?;
    assert_same_path_rename_contract(storage.as_ref()).await
}
