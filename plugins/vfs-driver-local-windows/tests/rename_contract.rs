#![cfg(target_os = "windows")]
mod common;

use ah_vfs::{application::port::Driver, testing::assert_same_path_rename_contract};
use vfs_driver_local_windows::WindowsLocalDriver;

#[tokio::test]
async fn local_same_path_rename_obeys_storage_contract() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let driver = WindowsLocalDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let instance = registry.create(common::local_config(dir.path())?).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    assert_same_path_rename_contract(storage.as_ref(), driver.info().capabilities().rename()).await
}

#[tokio::test]
async fn remove_unlinks_the_name_while_the_read_handle_is_open()
-> Result<(), Box<dyn std::error::Error>> {
    use ah_vfs_storage::{
        Failure, PathExpr, ReadRange, ReadRequest, Rejection, RemoveKind, RemoveRequest,
    };

    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("file"), b"open data")?;
    let driver = WindowsLocalDriver::new()?;
    let registry = common::registry(&driver, common::pool().await?).await?;
    let instance = registry.create(common::local_config(dir.path())?).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    let path = PathExpr::parse("/file")?;
    let mut reader = storage
        .read_file(
            ReadRequest::new(path.clone(), ReadRange::FULL),
            Default::default(),
        )
        .await?;
    assert!(
        matches!(storage.remove_entry(RemoveRequest::new(path.clone(), RemoveKind::EmptyDirectory)?, Default::default()).await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotDirectory))
    );
    storage
        .remove_entry(
            RemoveRequest::new(path, RemoveKind::NonDirectory)?,
            Default::default(),
        )
        .await?;
    assert!(!dir.path().join("file").exists());
    assert_eq!(reader.next_chunk().await?, Some(b"open data".to_vec()));
    Ok(())
}
