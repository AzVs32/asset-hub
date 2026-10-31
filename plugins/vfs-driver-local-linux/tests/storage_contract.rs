#![cfg(target_os = "linux")]
use ah_vfs::application::mount_runtime::MountRuntime;
mod common;

#[cfg(unix)]
use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        error::EntryError,
        service::{EntryService, MountService},
    },
    domain::{
        DriverInstanceRef, DriverKey, EntryPermissions, VirtualPath, XattrName, XattrWriteMode,
    },
    infrastructure::mount_repository::SqliteMountRepository,
};
use ah_vfs::{
    application::{
        dto::*,
        port::{Driver, Storage},
    },
    domain::{DriverError, FileReadSupport, InstanceId, StorageErrorKind, StoragePath},
};
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::num::NonZeroUsize;
use std::{error::Error, path::PathBuf, sync::Arc};
use vfs_driver_local_linux::LinuxLocalDriver;
type TestResult = Result<(), Box<dyn Error>>;
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    driver: Arc<LinuxLocalDriver>,
    id: InstanceId,
    storage: Arc<dyn Storage>,
    pool: sqlx::SqlitePool,
}
async fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("root");
    std::fs::create_dir(&root)?;
    std::fs::write(root.join("file"), b"original")?;
    std::fs::create_dir(root.join("dir"))?;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    let driver = Arc::new(LinuxLocalDriver::from_pool(pool.clone()).await?);
    let instance = driver.create_instance(common::local_config(&root)?).await?;
    let storage = driver.open(instance.id.clone().into()).await?;
    Ok(Fixture {
        _dir: dir,
        root,
        driver,
        id: instance.id.clone(),
        storage,
        pool,
    })
}
fn path(name: &str) -> Result<StoragePath, Box<dyn Error>> {
    Ok(StoragePath::parse(name)?)
}
#[cfg(unix)]
async fn service(f: &Fixture) -> Result<EntryService, Box<dyn Error>> {
    let repository = Arc::new(SqliteMountRepository::in_memory().await?);
    let mut catalog = DriverCatalog::new();
    ah_vfs::testing::register_driver(&mut catalog, f.driver.clone())?;
    let catalog = Arc::new(catalog);
    let runtime = Arc::new(MountRuntime::new(repository, catalog));
    MountService::new(runtime.clone())
        .initialize_root(DriverInstanceRef::new(
            DriverKey::parse("local")?,
            f.id.clone(),
        ))
        .await?;
    Ok(EntryService::new(runtime))
}

#[tokio::test]
async fn instance_support_matches_driver_and_atomic_publication_is_honored() -> TestResult {
    let f = fixture().await?;
    let caps = f.storage.capabilities();
    assert!(f.driver.info().capabilities().supports(caps));
    assert_eq!(caps.read_file, FileReadSupport::Range);
    assert_eq!(
        caps.permissions.can_read(),
        f.storage.permissions().is_some()
    );
    assert_eq!(caps.xattrs.can_read(), f.storage.xattrs().is_some());
    assert_eq!(
        caps.rename.no_replace,
        cfg!(any(target_os = "linux", target_os = "android"))
    );
    let mut request = StorageWriteFileRequest::new(path("new")?);
    request.require_atomic_publish = true;
    let mut writer = f.storage.write_file(request).await?;
    writer.write_chunk(b"new bytes").await?;
    assert!(!f.root.join("new").exists());
    writer.commit().await?;
    assert_eq!(std::fs::read(f.root.join("new"))?, b"new bytes");
    Ok(())
}

#[tokio::test]
async fn io_and_opening_errors_keep_categories_and_original_sources() -> TestResult {
    let f = fixture().await?;
    let error = f
        .storage
        .stat_entry(StorageStatEntryRequest {
            path: path("missing")?,
        })
        .await
        .err()
        .ok_or("expected missing entry")?;
    assert_eq!(error.kind(), StorageErrorKind::NotFound);
    assert!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<std::io::Error>())
            .is_some()
    );
    f.pool.close().await;
    let error = f
        .driver
        .open(f.id.clone().into())
        .await
        .err()
        .ok_or("expected closed pool failure")?;
    assert!(
        matches!(&error, DriverError::Storage(error) if error.kind() == StorageErrorKind::Unavailable)
    );
    assert!(error.source().is_some());
    assert_eq!(
        f.storage
            .stat_entry(StorageStatEntryRequest {
                path: path("file")?
            })
            .await?
            .size,
        Some(8)
    );
    Ok(())
}

#[tokio::test]
async fn unavailable_root_is_normalized_before_returning_from_driver_open() -> TestResult {
    let f = fixture().await?;
    std::fs::remove_dir_all(&f.root)?;
    let error = f
        .driver
        .open(f.id.clone().into())
        .await
        .err()
        .ok_or("expected missing root")?;
    assert!(
        matches!(&error, DriverError::Storage(error) if error.kind() == StorageErrorKind::NotFound)
    );
    assert!(error.source().is_some());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn permissions_round_trip_through_vfs_and_protect_mount_boundaries() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture().await?;
    let entries = service(&f).await?;
    for target in ["/file", "/dir"] {
        let target = VirtualPath::parse(target)?;
        entries
            .set_permissions(&target, EntryPermissions::new(0o750)?)
            .await?;
        assert_eq!(entries.get_permissions(&target).await?.bits(), 0o750);
    }
    assert_eq!(
        std::fs::metadata(f.root.join("file"))?.permissions().mode() & 0o7777,
        0o750
    );
    assert!(matches!(
        entries
            .set_permissions(&VirtualPath::root(), EntryPermissions::new(0o700)?)
            .await,
        Err(EntryError::MountBoundary(_))
    ));
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[tokio::test]
async fn xattrs_support_binary_empty_and_bounded_values_with_atomic_existence_policies()
-> TestResult {
    let f = fixture().await?;
    let entries = service(&f).await?;
    if !f.storage.capabilities().xattrs.can_read() {
        assert!(f.storage.xattrs().is_none());
        return Ok(());
    }
    let target = VirtualPath::parse("/file")?;
    let name = XattrName::parse("user.asset-hub-test")?;
    let limit = NonZeroUsize::new(4096).ok_or("zero limit")?;
    entries
        .set_xattr(
            &target,
            name.clone(),
            vec![0, 255, 1, 0],
            XattrWriteMode::CreateNew,
        )
        .await?;
    assert_eq!(
        entries.get_xattr(&target, name.clone(), limit).await?,
        [0, 255, 1, 0]
    );
    assert!(
        matches!(entries.set_xattr(&target, name.clone(), vec![], XattrWriteMode::CreateNew).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::AlreadyExists))
    );
    assert!(
        matches!(entries.get_xattr(&target, name.clone(), NonZeroUsize::MIN).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::TooLarge))
    );
    assert!(
        matches!(entries.list_xattrs(&target, NonZeroUsize::MIN).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::TooLarge))
    );
    let names = entries.list_xattrs(&target, limit).await?;
    assert!(names.contains(&name));
    assert!(
        names
            .windows(2)
            .all(|pair| pair[0].as_str() < pair[1].as_str())
    );
    entries
        .set_xattr(&target, name.clone(), vec![], XattrWriteMode::Replace)
        .await?;
    assert!(
        entries
            .get_xattr(&target, name.clone(), limit)
            .await?
            .is_empty()
    );
    entries.remove_xattr(&target, name.clone()).await?;
    assert!(
        matches!(entries.get_xattr(&target, name.clone(), limit).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::AttributeNotFound))
    );
    assert!(
        matches!(entries.set_xattr(&target, name.clone(), vec![1], XattrWriteMode::Replace).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::AttributeNotFound))
    );
    assert!(
        matches!(entries.remove_xattr(&target, name.clone()).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::AttributeNotFound))
    );
    let dir = VirtualPath::parse("/dir")?;
    entries
        .set_xattr(&dir, name.clone(), vec![2], XattrWriteMode::Upsert)
        .await?;
    assert_eq!(entries.get_xattr(&dir, name.clone(), limit).await?, [2]);
    assert!(matches!(
        entries
            .set_xattr(&VirtualPath::root(), name, vec![3], XattrWriteMode::Upsert)
            .await,
        Err(EntryError::MountBoundary(_))
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn attribute_access_never_follows_file_or_parent_symlinks() -> TestResult {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let f = fixture().await?;
    let outside = tempfile::tempdir()?;
    let outside_file = outside.path().join("file");
    std::fs::write(&outside_file, b"outside")?;
    let mode = std::fs::metadata(&outside_file)?.permissions().mode();
    symlink(&outside_file, f.root.join("link"))?;
    symlink(outside.path(), f.root.join("linked-dir"))?;
    let permissions = f
        .storage
        .permissions()
        .ok_or("missing permissions extension")?;
    for name in ["link", "linked-dir/file"] {
        assert!(permissions.get_permissions(path(name)?).await.is_err());
        assert!(
            permissions
                .set_permissions(path(name)?, EntryPermissions::new(0o700)?)
                .await
                .is_err()
        );
        if let Some(xattrs) = f.storage.xattrs() {
            assert!(
                xattrs
                    .set_xattr(
                        path(name)?,
                        XattrName::parse("user.asset-hub-test")?,
                        vec![1],
                        XattrWriteMode::Upsert
                    )
                    .await
                    .is_err()
            );
        }
    }
    assert_eq!(std::fs::metadata(outside_file)?.permissions().mode(), mode);
    Ok(())
}
