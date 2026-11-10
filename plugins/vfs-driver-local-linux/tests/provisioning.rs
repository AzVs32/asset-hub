#![cfg(target_os = "linux")]
use ah_vfs::application::driver_error::LifecycleFailure;
mod common;
use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::dto::*;
use ah_vfs::application::port::*;
use ah_vfs_storage::StoragePath;
use vfs_driver_local_linux::LinuxLocalDriver;
type Result = std::result::Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn prepare_validates_fields_and_normalizes_root() -> Result {
    let driver = LinuxLocalDriver::new()?;
    let provisioner = driver.provisioner();
    for config in [
        InstanceConfig::default(),
        InstanceConfig::from([("root_path".into(), 42.into())]),
        InstanceConfig::from([("unknown".into(), true.into())]),
        InstanceConfig::from([("root_path".into(), "".into())]),
    ] {
        assert!(matches!(
            provisioner.prepare(config).await,
            Err(DriverError::InvalidInstanceConfig { .. })
        ));
    }
    let relative = PathBuf::from(".");
    let prepared = provisioner
        .prepare(common::local_config(&relative)?)
        .await?;
    let expected = std::fs::canonicalize(relative)?;
    assert!(expected.is_absolute());
    assert_eq!(prepared.config()["root_path"].as_str(), expected.to_str());
    assert_eq!(prepared.details()["root_path"].as_str(), expected.to_str());
    Ok(())
}
#[tokio::test]
async fn host_creation_opens_for_read_write_and_rejects_root_changes() -> Result {
    let driver = LinuxLocalDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let root = tempfile::tempdir()?;
    let another = tempfile::tempdir()?;
    let instance = registry.create(common::local_config(root.path())?).await?;
    assert!(
        matches!(registry.update(instance.id.clone(),common::local_config(another.path())?).await,Err(DriverError::Lifecycle(error)) if error.failure()==LifecycleFailure::InvalidRequest)
    );
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    let path = StoragePath::parse("file")?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path.clone().to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"data").await?;
    writer.commit().await?;
    let mut reader = storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                (path).to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(reader.next_chunk().await?.ok_or("chunk")?, b"data");
    assert!(reader.next_chunk().await?.is_none());
    assert_eq!(
        registry.remove(instance.id.clone()).await?,
        RemoveInstanceOutcome::Removed
    );
    assert!(registry.get(instance.id.clone()).await?.is_none());
    assert!(matches!(
        registry.open_context(instance.id.clone()).await,
        Err(DriverError::InstanceNotFound(_))
    ));
    assert_eq!(
        registry.remove(instance.id).await?,
        RemoveInstanceOutcome::AlreadyAbsent
    );
    Ok(())
}
use std::path::PathBuf;
