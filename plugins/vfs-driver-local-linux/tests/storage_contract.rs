#![cfg(target_os = "linux")]
use ah_vfs::application::mount_runtime::MountRuntime;
use ah_vfs_storage::{Failure, Rejection};
mod common;

#[cfg(unix)]
use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::dto::{MountRequest, ReadFileRequest};
use ah_vfs::application::error::EntryError;
use ah_vfs::application::port::{Driver, LookupContext, Storage, validate_storage_extensions};
use ah_vfs::application::service::{EntryService, MountService};
use ah_vfs::domain::namespace::VirtualPath;
use ah_vfs::domain::{DriverInstance, DriverKey, InstanceId};
use ah_vfs::infrastructure::mount_repository::SqliteMountRepository;
use ah_vfs_storage::{
    AttributeAction as A, AttributeRequest, AttributeResult as R, BackendFailure,
    IntermediateSymlinks, MutationOutcome, PathExpr, Permissions, StoragePath, XattrName,
    XattrWriteMode,
};
use ah_vfs_storage::{ReadRange, ReadSupport, RenameMode};

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
    validate_storage_extensions(caps, f.storage.as_ref())?;
    assert_eq!(
        caps.rename().supports(RenameMode::NoReplace),
        cfg!(any(target_os = "linux", target_os = "android"))
    );
    let mut writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("new")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Atomic,
            ),
            Default::default(),
        )
        .await?;
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
                path("missing")?.to_rooted_expr(),
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
                    path("file")?.to_rooted_expr(),
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

#[tokio::test]
async fn permissions_round_trip_through_vfs_and_protect_mount_boundaries() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture().await?;
    let (entries, _) = services(&f).await?;
    let mode = Permissions::new(0o750)?;
    for target in ["/file", "/dir/"] {
        assert_eq!(
            entries.attribute(target, A::SetPermissions(mode)).await?,
            R::Unit
        );
        assert_eq!(
            entries.attribute(target, A::GetPermissions).await?,
            R::Permissions(mode)
        );
    }
    assert_eq!(
        std::fs::metadata(f.root.join("file"))?.permissions().mode() & 0o7777,
        0o750
    );
    assert!(matches!(
        entries.attribute("/", A::SetPermissions(mode)).await,
        Err(EntryError::MountBoundary(_))
    ));
    Ok(())
}

#[tokio::test]
async fn xattrs_support_binary_empty_and_bounded_values_with_atomic_existence_policies()
-> TestResult {
    let f = fixture().await?;
    let (entries, _) = services(&f).await?;
    let name = XattrName::parse("user.asset-hub-test")?;
    let limit = NonZeroUsize::new(4096).ok_or("zero limit")?;
    let get = || A::get_xattr(name.clone(), limit);
    let set = |value, mode| A::set_xattr(name.clone(), value, mode);

    let first = entries
        .attribute("/file", set(vec![0, 255, 1, 0], XattrWriteMode::CreateNew))
        .await;
    if first
        .as_ref()
        .err()
        .and_then(EntryError::storage_error)
        .is_some_and(|error| error.failure() == Failure::Rejected(Rejection::Unsupported))
    {
        return Ok(());
    }
    assert_eq!(first?, R::Unit);
    assert_eq!(
        entries.attribute("/file", get()).await?,
        R::Value(vec![0, 255, 1, 0])
    );
    for (action, reason) in [
        (
            set(vec![], XattrWriteMode::CreateNew),
            Rejection::AlreadyExists,
        ),
        (
            A::get_xattr(name.clone(), NonZeroUsize::MIN),
            Rejection::LimitExceeded,
        ),
        (A::list_xattrs(NonZeroUsize::MIN), Rejection::LimitExceeded),
        (
            set(vec![0; 64 * 1024 + 1], XattrWriteMode::Upsert),
            Rejection::LimitExceeded,
        ),
    ] {
        let error = entries
            .attribute("/file", action)
            .await
            .err()
            .ok_or("expected rejection")?;
        let error = error.storage_error().ok_or("expected storage error")?;
        assert_eq!(error.failure(), Failure::Rejected(reason));
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    }

    let first_name = XattrName::parse("user.a-first")?;
    entries
        .attribute(
            "/file",
            A::set_xattr(first_name.clone(), vec![7], XattrWriteMode::Upsert),
        )
        .await?;
    let R::Names(names) = entries.attribute("/file", A::list_xattrs(limit)).await? else {
        return Err("expected names".into());
    };
    assert_eq!(names.names(), &[first_name, name.clone()]);

    entries
        .attribute("/file", set(vec![], XattrWriteMode::Replace))
        .await?;
    assert_eq!(entries.attribute("/file", get()).await?, R::Value(vec![]));
    entries
        .attribute("/file", A::RemoveXattr(name.clone()))
        .await?;
    for action in [
        get(),
        set(vec![1], XattrWriteMode::Replace),
        A::RemoveXattr(name.clone()),
    ] {
        let error = entries
            .attribute("/file", action)
            .await
            .err()
            .ok_or("expected missing attribute")?;
        let error = error.storage_error().ok_or("expected storage error")?;
        assert_eq!(
            error.failure(),
            Failure::Rejected(Rejection::AttributeNotFound)
        );
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    }

    let (first, second) = tokio::join!(
        entries.attribute("/file", set(vec![1], XattrWriteMode::CreateNew)),
        entries.attribute("/file", set(vec![2], XattrWriteMode::CreateNew)),
    );
    let (winner, loser) = match (first, second) {
        (Ok(R::Unit), Err(error)) => (1, error),
        (Err(error), Ok(R::Unit)) => (2, error),
        _ => return Err("expected one atomic create".into()),
    };
    let loser = loser.storage_error().ok_or("expected storage error")?;
    assert_eq!(loser.failure(), Failure::Rejected(Rejection::AlreadyExists));
    assert_eq!(loser.outcome(), MutationOutcome::Unchanged);
    assert_eq!(
        entries.attribute("/file", get()).await?,
        R::Value(vec![winner])
    );

    entries
        .attribute("/dir/", set(vec![2], XattrWriteMode::Upsert))
        .await?;
    assert_eq!(entries.attribute("/dir/", get()).await?, R::Value(vec![2]));
    assert!(matches!(
        entries
            .attribute("/", set(vec![3], XattrWriteMode::Upsert))
            .await,
        Err(EntryError::MountBoundary(_))
    ));

    let invalid = std::ffi::CStr::from_bytes_with_nul(b"user.native-\xff\0")?;
    rustix::fs::setxattr(
        f.root.join("file"),
        invalid,
        b"value",
        rustix::fs::XattrFlags::CREATE,
    )?;
    let error = entries
        .attribute("/file", A::list_xattrs(limit))
        .await
        .err()
        .ok_or("invalid native name accepted")?;
    assert_eq!(
        error.storage_error().ok_or("storage error")?.failure(),
        Failure::Backend(BackendFailure::UnrepresentableName)
    );
    Ok(())
}

#[tokio::test]
async fn attribute_lookup_preserves_the_final_link_and_obeys_intermediate_policy() -> TestResult {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let f = fixture().await?;
    let outside = tempfile::tempdir()?;
    let outside_file = outside.path().join("file");
    std::fs::write(&outside_file, b"outside")?;
    let outside_mode = outside_file.metadata()?.permissions().mode();
    std::fs::set_permissions(f.root.join("file"), std::fs::Permissions::from_mode(0o600))?;
    symlink(&outside_file, f.root.join("link"))?;
    symlink(outside.path(), f.root.join("outside"))?;
    symlink("/", f.root.join("alias"))?;
    let attributes = f.storage.attributes().ok_or("missing attributes")?;
    let execute = |operand: &str, action, intermediate_symlinks| -> Result<_, Box<dyn Error>> {
        Ok(attributes.execute(
            AttributeRequest::new(PathExpr::parse(operand)?, action),
            LookupContext {
                working_directory: path("dir")?,
                intermediate_symlinks,
            },
        ))
    };
    use IntermediateSymlinks::{FollowWithinInstance, Reject};
    assert_eq!(
        execute("../alias/file", A::GetPermissions, FollowWithinInstance)?.await?,
        R::Permissions(Permissions::new(0o600)?)
    );
    assert_eq!(
        execute("../link", A::GetPermissions, FollowWithinInstance)?.await?,
        R::Permissions(Permissions::new(0o777)?)
    );

    for (operand, action, links, reason) in [
        (
            "../alias/file",
            A::GetPermissions,
            Reject,
            Rejection::SymlinkNotAllowed,
        ),
        (
            "/link/",
            A::GetPermissions,
            FollowWithinInstance,
            Rejection::NotDirectory,
        ),
        (
            "/outside/file",
            A::GetPermissions,
            FollowWithinInstance,
            Rejection::NotFound,
        ),
        (
            "/link",
            A::SetPermissions(Permissions::new(0o700)?),
            FollowWithinInstance,
            Rejection::Unsupported,
        ),
        (
            "/link",
            A::list_xattrs(NonZeroUsize::MIN),
            FollowWithinInstance,
            Rejection::Unsupported,
        ),
    ] {
        let error = execute(operand, action, links)?
            .await
            .err()
            .ok_or("expected lookup rejection")?;
        assert_eq!(error.failure(), Failure::Rejected(reason));
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    }
    assert_eq!(outside_file.metadata()?.permissions().mode(), outside_mode);
    Ok(())
}

#[tokio::test]
async fn instance_lookup_does_not_route_again_through_child_mounts() -> TestResult {
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
    let mut reader = entries
        .read_file(ReadFileRequest {
            path: VirtualPath::parse("/alias/file")?,
            range: ReadRange::new(9, Some(7))?,
        })
        .await?;
    assert_eq!(reader.next_chunk().await?, Some(b"in root".to_vec()));
    assert_eq!(reader.next_chunk().await?, None);
    assert_eq!(reader.next_chunk().await?, None);
    let target = entries.read_link(&VirtualPath::parse("/alias")?).await?;
    assert_eq!(target.as_bytes(), b"/mounted");
    assert!(matches!(
        entries.read_link(&VirtualPath::parse("/mounted")?).await,
        Err(EntryError::Storage(error)) if error.failure() == Failure::Rejected(Rejection::NotSymlink)
    ));
    Ok(())
}
