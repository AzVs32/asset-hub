mod common;

use ah_vfs::{application::port::Driver, testing::assert_range_read_contract};
use vfs_driver_local::LocalDriver;

#[tokio::test]
async fn local_range_reads_obey_storage_contract() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let driver = LocalDriver::from_pool(common::pool().await?).await?;
    let instance = driver
        .create_instance(common::local_config(dir.path())?)
        .await?;
    let storage = driver.open(instance.id.clone().into()).await?;
    assert_range_read_contract(storage.as_ref()).await
}
