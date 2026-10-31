mod common;

use ah_vfs::{application::port::Driver, testing::assert_pagination_contract};
use common::{TestResult, pool};
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

#[tokio::test]
async fn memory_uses_shared_pagination_validation() -> TestResult {
    let driver = MemoryDriver::from_pool(pool().await?).await?;
    let instance = driver
        .create_instance(common::memory_config(1, CapacityUnit::KiB))
        .await?;
    let storage = driver.open(instance.id.clone().into()).await?;
    assert_pagination_contract(storage.as_ref()).await
}
