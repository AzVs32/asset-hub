mod common;

use ah_vfs::{application::port::Driver, testing::assert_range_read_contract};
use common::TestResult;
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

#[tokio::test]
async fn memory_range_reads_obey_storage_contract() -> TestResult {
    let driver = MemoryDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let instance = registry
        .create(common::memory_config(1, CapacityUnit::MiB))
        .await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    assert_range_read_contract(storage.as_ref()).await
}
