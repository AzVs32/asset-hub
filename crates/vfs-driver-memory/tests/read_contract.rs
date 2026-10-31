mod common;

use ah_vfs::{application::port::Driver, testing::assert_range_read_contract};
use common::{TestResult, pool};
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

#[tokio::test]
async fn memory_range_reads_obey_storage_contract() -> TestResult {
    let driver = MemoryDriver::from_pool(pool().await?).await?;
    let instance = driver
        .create_instance(common::memory_config(1, CapacityUnit::MiB))
        .await?;
    let storage = driver.open(instance.id.clone().into()).await?;
    assert_range_read_contract(storage.as_ref()).await
}
