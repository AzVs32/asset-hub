#![cfg(target_os = "linux")]
use ah_vfs::application::mount_runtime::MountRuntime;
use ah_vfs_storage::{Failure, Rejection};
mod common;

#[cfg(unix)]
use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::dto::MountRequest;
use ah_vfs::application::error::EntryError;
use ah_vfs::application::port::{Driver, Storage};
use ah_vfs::application::service::{EntryService, MountService};
use ah_vfs::domain::namespace::VirtualPath;
use ah_vfs::domain::storage::StorageWriteFileRequest;
use ah_vfs::domain::{DriverInstance, DriverKey, InstanceId};
use ah_vfs::infrastructure::mount_repository::SqliteMountRepository;
use ah_vfs_storage::{Permissions, StoragePath, XattrName, XattrWriteMode};
use ah_vfs_storage::{ReadSupport, RenameMode};

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
    registry: ah_vfs::application::host_instance_registry::HostInstanceRegistry,
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
    let driver = Arc::new(LinuxLocalDriver::new()?);
    let registry = common::registry(driver.as_ref(), pool.clone()).await?;
    let instance = registry.create(common::local_config(&root)?).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    Ok(Fixture {
        _dir: dir,
        root,
        driver,
        id: instance.id.clone(),
        registry,
        storage,
        pool,
    })
}
fn path(name: &str) -> Result<StoragePath, Box<dyn Error>> {
    Ok(StoragePath::parse(name)?)
}
#[cfg(unix)]
async fn services(f: &Fixture) -> Result<(EntryService, MountService), Box<dyn Error>> {
    let repository = Arc::new(SqliteMountRepository::in_memory().await?);
    let mut catalog = DriverCatalog::new();
    ah_vfs::testing::register_driver(&mut catalog, f.driver.clone())?;
    let catalog = Arc::new(catalog);
    let runtime = Arc::new(MountRuntime::new(repository, catalog, Arc::new(ah_vfs::infrastructure::instance_record_repository::SqliteInstanceRecordRepository::from_pool(f.pool.clone()).await?), None));
    let mounts = MountService::new(runtime.clone());
    mounts
        .initialize_root(DriverInstance::new(
            DriverKey::parse("local")?,
            f.id.clone(),
        ))
        .await?;
    Ok((EntryService::new(runtime), mounts))
}

#[tokio::test]
async fn instance_support_matches_driver_and_atomic_publication_is_honored() -> TestResult {
    let f = fixture().await?;
    let caps = f.driver.info().capabilities();
    assert_eq!(caps.read(), ReadSupport::Range);
    assert_eq!(
        caps.permissions().can_read(),
        f.storage.permissions().is_some()
    );
    assert_eq!(caps.xattrs().can_read(), f.storage.xattrs().is_some());
    assert_eq!(
        caps.rename().supports(RenameMode::NoReplace),
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
        .stat_entry(
            ah_vfs_storage::StatRequest::new(
                (path("missing")?).to_rooted_expr(),
                ah_vfs_storage::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await
        .err()
        .ok_or("expected missing entry")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::NotFound));
    assert!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<std::io::Error>())
            .is_some()
    );
    f.pool.close().await;
    let error = f
        .registry
        .open_context(f.id.clone())
        .await
        .err()
        .ok_or("expected closed pool failure")?;
    assert!(
        matches!(&error, DriverError::Lifecycle(error) if error.failure() == ah_vfs::application::driver_error::LifecycleFailure::Unavailable)
    );
    assert!(error.source().is_some());
    assert_eq!(
        f.storage
            .stat_entry(
                ah_vfs_storage::StatRequest::new(
                    (path("file")?).to_rooted_expr(),
                    ah_vfs_storage::SymlinkAction::Preserve
                ),
                Default::default()
            )
            .await?
            .entry_type(),
        ah_vfs_storage::EntryType::file(Some(8))
    );
    Ok(())
}

#[tokio::test]
async fn unavailable_root_is_normalized_before_returning_from_driver_open() -> TestResult {
    let f = fixture().await?;
    std::fs::remove_dir_all(&f.root)?;
    let error = f
        .driver
        .open(f.registry.open_context(f.id.clone()).await?)
        .await
        .err()
        .ok_or("expected missing root")?;
    assert!(
        matches!(&error, DriverError::Lifecycle(error) if error.failure() == ah_vfs::application::driver_error::LifecycleFailure::Backend)
    );
    assert!(error.source().is_some());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn permissions_round_trip_through_vfs_and_protect_mount_boundaries() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture().await?;
    let (entries, _) = services(&f).await?;
    for target in ["/file", "/dir"] {
        let target = VirtualPath::parse(target)?;
        entries
            .set_permissions(&target, Permissions::new(0o750)?)
            .await?;
        assert_eq!(entries.get_permissions(&target).await?.bits(), 0o750);
    }
    assert_eq!(
        std::fs::metadata(f.root.join("file"))?.permissions().mode() & 0o7777,
        0o750
    );
    assert!(matches!(
        entries
            .set_permissions(&VirtualPath::root(), Permissions::new(0o700)?)
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
    let (entries, _) = services(&f).await?;
    let target = VirtualPath::parse("/file")?;
    let name = XattrName::parse("user.asset-hub-test")?;
    let limit = NonZeroUsize::new(4096).ok_or("zero limit")?;
    let result = entries
        .set_xattr(
            &target,
            name.clone(),
            vec![0, 255, 1, 0],
            XattrWriteMode::CreateNew,
        )
        .await;
    if result.as_ref().err().is_some_and(|error| {
        error
            .storage_error()
            .is_some_and(|error| error.failure() == Failure::Rejected(Rejection::Unsupported))
    }) {
        assert!(f.storage.xattrs().is_some());
        return Ok(());
    }
    result?;
    assert_eq!(
        entries.get_xattr(&target, name.clone(), limit).await?,
        [0, 255, 1, 0]
    );
    assert!(
        matches!(entries.set_xattr(&target, name.clone(), vec![], XattrWriteMode::CreateNew).await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::AlreadyExists)))
    );
    assert!(
        matches!(entries.get_xattr(&target, name.clone(), NonZeroUsize::MIN).await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::LimitExceeded)))
    );
    assert!(
        matches!(entries.list_xattrs(&target, NonZeroUsize::MIN).await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::LimitExceeded)))
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
        matches!(entries.get_xattr(&target, name.clone(), limit).await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::AttributeNotFound)))
    );
    assert!(
        matches!(entries.set_xattr(&target, name.clone(), vec![1], XattrWriteMode::Replace).await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::AttributeNotFound)))
    );
    assert!(
        matches!(entries.remove_xattr(&target, name.clone()).await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::AttributeNotFound)))
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
                .set_permissions(path(name)?, Permissions::new(0o700)?)
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

#[tokio::test]
async fn stat_and_read_link_do_not_route_again_through_child_mounts() -> TestResult {
    let f = fixture().await?;
    let (entries, mounts) = services(&f).await?;
    let other_root = f._dir.path().join("other");
    std::fs::create_dir(&other_root)?;
    std::fs::write(other_root.join("file"), b"mounted")?;
    let other = f
        .registry
        .create(common::local_config(&other_root)?)
        .await?;
    mounts
        .mount(MountRequest {
            virtual_path: VirtualPath::parse("/mounted")?,
            driver_instance: DriverInstance::new(DriverKey::parse("local")?, other.id.clone()),
        })
        .await?;

    std::fs::create_dir(f.root.join("mounted"))?;
    std::fs::write(f.root.join("mounted/file"), b"original in root instance")?;
    std::os::unix::fs::symlink("/mounted", f.root.join("alias"))?;
    let via_alias = entries
        .stat_entry(&VirtualPath::parse("/alias/file")?)
        .await?;
    let via_mount = entries
        .stat_entry(&VirtualPath::parse("/mounted/file")?)
        .await?;
    assert_eq!(via_alias.kind, ah_vfs_storage::EntryType::file(Some(25)));
    assert_eq!(via_mount.kind, ah_vfs_storage::EntryType::file(Some(7)));
    let target = entries.read_link(&VirtualPath::parse("/alias")?).await?;
    assert_eq!(target.as_bytes(), b"/mounted");
    assert!(matches!(
        entries.read_link(&VirtualPath::parse("/mounted")?).await,
        Err(EntryError::Storage(error)) if error.failure() == Failure::Rejected(Rejection::NotSymlink)
    ));
    Ok(())
}
