use ah_vfs::application::mount_runtime::MountRuntime;
use ah_vfs_storage::{ContractViolation, Failure, Operation, Rejection, StorageError};
mod common;

use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::*;
use ah_vfs::application::error::EntryError;
use ah_vfs::application::port::{
    DriverFuture, Storage, StorageFileReader, StorageFileWriter, StorageFuture,
};
use ah_vfs::application::service::{EntryService, MountService};
use ah_vfs::domain::namespace::{ChildMountPolicy, VirtualPath};
use ah_vfs::domain::{DriverInstance, DriverKey, InstanceId};
use ah_vfs::infrastructure::mount_repository::SqliteMountRepository;
use ah_vfs_storage::{
    BackendFailure, BoundListRequest, DirectoryIdentity, DirectoryPage, DirectorySelection, Entry,
    EntryType, ListRequest, PageSize, StoragePath,
};

use std::{
    error::Error,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy)]
enum Response {
    Normal,
    WrongBinding,
    RetiredAfterScan,
}

struct TestDriver {
    info: DriverInfo,
    response: Response,
}
struct TestStorage {
    instance: InstanceId,
    response: Response,
    retired: AtomicBool,
}

fn unsupported<T: Send + 'static>(operation: Operation) -> StorageFuture<'static, T> {
    Box::pin(async move {
        Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::Unsupported),
            None,
        ))
    })
}

impl common::Backend for TestDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn open(
        &self,
        context: ah_vfs::application::port::OpenContext,
    ) -> DriverFuture<'_, Arc<dyn Storage>> {
        let instance = context.instance_id().clone();
        Box::pin(async move {
            Ok(Arc::new(TestStorage {
                instance,
                response: self.response,
                retired: AtomicBool::new(false),
            }) as Arc<dyn Storage>)
        })
    }
}

fn invalid_metadata(
    operation: Operation,
    cause: impl Error + Send + Sync + 'static,
) -> StorageError {
    StorageError::without_effects(
        operation,
        Failure::Contract(ContractViolation::InvalidResponse),
        Some(Box::new(cause)),
    )
}

impl Storage for TestStorage {
    fn is_stale(&self) -> bool {
        self.retired.load(Ordering::Relaxed)
    }
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn stat_entry(
        &self,
        request: ah_vfs_storage::StatRequest,
        _: ah_vfs::application::port::LookupContext,
    ) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let operand = request.path().to_string();
            let path = StoragePath::parse(operand.trim_start_matches('/')).map_err(|cause| {
                StorageError::without_effects(
                    Operation::Stat,
                    Failure::Contract(ContractViolation::InvalidEntry),
                    Some(Box::new(cause)),
                )
            })?;
            let kind = if path.is_root() {
                EntryType::Directory
            } else {
                EntryType::file(Some(if self.instance.as_str() == "leaf" {
                    42
                } else {
                    7
                }))
            };
            Entry::new(path, kind, None).map_err(|cause| invalid_metadata(Operation::Stat, cause))
        })
    }
    fn select_directory<'a>(
        &'a self,
        _: &'a ListRequest,
        _: ah_vfs::application::port::LookupContext,
    ) -> StorageFuture<'a, DirectorySelection> {
        Box::pin(async {
            Ok(DirectorySelection::from_lookup(
                StoragePath::root(),
                DirectoryIdentity::from_bytes([1; 16]),
            ))
        })
    }

    fn list_page<'a>(
        &'a self,
        bound: &'a BoundListRequest<'_>,
    ) -> StorageFuture<'a, DirectoryPage> {
        Box::pin(async move {
            if matches!(self.response, Response::RetiredAfterScan) && bound.after().is_some() {
                self.retired.store(true, Ordering::Relaxed);
            }
            if matches!(self.response, Response::WrongBinding) {
                let request = ListRequest::new(
                    ah_vfs_storage::ListTarget::Path(StoragePath::root().to_rooted_expr()),
                    bound.size(),
                );
                let wrong = DirectorySelection::from_lookup(
                    StoragePath::root(),
                    DirectoryIdentity::from_bytes([2; 16]),
                );
                let wrong = request.bind_selection(wrong).map_err(|reason| {
                    StorageError::without_effects(Operation::List, Failure::Backend(reason), None)
                })?;
                return DirectoryPage::new(&wrong, vec![], false)
                    .map_err(|cause| invalid_metadata(Operation::List, cause));
            }
            let mut names = ["a", "docs", "z"]
                .into_iter()
                .filter(|name| bound.after().is_none_or(|after| *name > after.as_str()));
            // A backend can deliberately return short pages.
            let entries = names
                .next()
                .map(|name| {
                    let path = StoragePath::parse(name)
                        .map_err(|cause| invalid_metadata(Operation::List, cause))?;
                    Entry::new(path, EntryType::file(Some(1)), None)
                        .map_err(|cause| invalid_metadata(Operation::List, cause))
                })
                .transpose()?
                .into_iter()
                .collect();
            DirectoryPage::new(bound, entries, names.next().is_some())
                .map_err(|cause| invalid_metadata(Operation::List, cause))
        })
    }
    fn read_file(
        &self,
        _: ah_vfs_storage::ReadRequest,
        _: ah_vfs::application::port::LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        unsupported(Operation::Read)
    }
    fn write_file(
        &self,
        _: ah_vfs_storage::WriteRequest,
        _: ah_vfs::application::port::LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        unsupported(Operation::Write)
    }
}

async fn services(response: Response) -> TestResult<(EntryService, MountService)> {
    let key = DriverKey::parse("fake")?;
    let mut drivers = DriverCatalog::new();
    common::register(
        &mut drivers,
        common::managed(Arc::new(TestDriver {
            info: DriverInfo::builder()
                .capabilities(
                    ah_vfs_storage::Capabilities::default()
                        .with_stat(true)
                        .with_list(true),
                )
                .key(key.clone())
                .name("Test".into())
                .version(semver::Version::new(0, 1, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .build(),
            response,
        })),
    )?;
    let drivers = Arc::new(drivers);
    let repository = Arc::new(SqliteMountRepository::in_memory().await?);
    let runtime = Arc::new({
        let drivers = drivers;
        let records = common::Records::seeded(&drivers, &["root", "leaf"])?;
        MountRuntime::new(repository, drivers, records, None)
    });
    let files = EntryService::new(runtime.clone());
    let mounts = MountService::new(runtime);
    mounts
        .initialize_root(DriverInstance::new(key, InstanceId::parse("root")?))
        .await?;
    Ok((files, mounts))
}

#[tokio::test]
async fn resolves_the_deepest_driver_instance_and_maps_metadata_to_virtual_paths() -> TestResult {
    let (files, mounts) = services(Response::Normal).await?;
    mounts
        .mount(MountRequest {
            virtual_path: VirtualPath::parse("/docs")?,
            driver_instance: DriverInstance::new(
                DriverKey::parse("fake")?,
                InstanceId::parse("leaf")?,
            ),
        })
        .await?;
    let entry = files
        .stat_entry(&VirtualPath::parse("/docs/inside")?)
        .await?;
    assert_eq!(entry.path.as_str(), "/docs/inside");
    assert_eq!(entry.kind, EntryType::file(Some(42)));
    assert_eq!(
        files
            .stat_entry(&VirtualPath::parse("/docs-old")?)
            .await?
            .kind,
        EntryType::file(Some(7))
    );
    Ok(())
}

#[tokio::test]
async fn namespace_pagination_keeps_overlay_order_and_validates_virtual_checkpoints() -> TestResult
{
    let (files, mounts) = services(Response::Normal).await?;
    let mounted = mounts
        .mount(MountRequest {
            virtual_path: VirtualPath::parse("/docs")?,
            driver_instance: DriverInstance::new(
                DriverKey::parse("fake")?,
                InstanceId::parse("leaf")?,
            ),
        })
        .await?;
    let root = VirtualPath::root();
    let size = PageSize::new(1)?;
    let first = files
        .list_entries(&root, DirQuery { cursor: None, size })
        .await?;
    assert_eq!(first.entries[0].path.as_str(), "/a");
    let cursor = first.continuation.ok_or("expected virtual checkpoint")?;
    assert!(
        matches!(files.list_entries(&VirtualPath::parse("/docs")?, DirQuery { cursor: Some(cursor.clone()), size }).await,
        Err(EntryError::Storage(error)) if error.failure() == Failure::Backend(BackendFailure::Stale))
    );
    let second = files
        .list_entries(
            &root,
            DirQuery {
                cursor: Some(cursor.clone()),
                size,
            },
        )
        .await?;
    assert_eq!(second.entries[0].path.as_str(), "/docs");
    assert_eq!(
        second.entries[0].origin,
        EntryOrigin::MountPoint { enabled: true }
    );
    let third = files
        .list_entries(
            &root,
            DirQuery {
                cursor: second.continuation,
                size,
            },
        )
        .await?;
    assert_eq!(third.entries[0].path.as_str(), "/z");
    assert!(third.continuation.is_none());
    mounts.disable(mounted.id).await?;
    assert!(
        matches!(files.list_entries(&root, DirQuery { cursor: Some(cursor), size }).await,
        Err(EntryError::Storage(error)) if error.failure() == Failure::Backend(BackendFailure::Stale))
    );
    Ok(())
}

#[tokio::test]
async fn virtual_cursor_rejects_a_retired_instance() -> TestResult {
    let (files, _) = services(Response::RetiredAfterScan).await?;
    let root = VirtualPath::root();
    let size = PageSize::new(1)?;
    let first = files
        .list_entries(&root, DirQuery { cursor: None, size })
        .await?;
    let cursor = first.continuation.ok_or("expected continuation")?;
    assert!(
        matches!(files.list_entries(&root, DirQuery { cursor: Some(cursor), size }).await,
        Err(EntryError::Storage(error)) if error.failure() == Failure::Backend(BackendFailure::Stale))
    );
    Ok(())
}

#[tokio::test]
async fn page_from_another_binding_is_rejected() -> TestResult {
    let (files, _) = services(Response::WrongBinding).await?;
    assert!(matches!(
        files
            .list_entries(&VirtualPath::root(), Default::default())
            .await,
        Err(EntryError::InvalidStorageResponse(_))
    ));
    Ok(())
}

#[tokio::test]
async fn mount_points_and_synthetic_dirs_keep_distinct_namespace_behavior() -> TestResult {
    let (files, mounts) = services(Response::Normal).await?;
    let nested = VirtualPath::parse("/docs/nested")?;
    let mounted = mounts
        .mount(MountRequest {
            virtual_path: nested.clone(),
            driver_instance: DriverInstance::new(
                DriverKey::parse("fake")?,
                InstanceId::parse("leaf")?,
            ),
        })
        .await?;
    let ancestor = VirtualPath::parse("/docs")?;
    let root = files.stat_entry(&VirtualPath::root()).await?;
    assert_eq!(root.origin, EntryOrigin::MountPoint { enabled: true });
    assert_eq!(root.kind, EntryType::Directory);
    let synthetic = files.stat_entry(&ancestor).await?;
    assert_eq!(synthetic.origin, EntryOrigin::SyntheticDir);
    assert_eq!(synthetic.kind, EntryType::Directory);
    let leaf = files.stat_entry(&nested).await?;
    assert_eq!(leaf.origin, EntryOrigin::MountPoint { enabled: true });
    assert_eq!(leaf.kind, EntryType::Directory);

    for path in [VirtualPath::root(), ancestor.clone(), nested.clone()] {
        let error = files
            .read_file(ReadFileRequest {
                path: path.clone(),
                range: ah_vfs_storage::ReadRange::new(0, None)?,
            })
            .await
            .err()
            .ok_or("namespace dir is not a file")?;
        assert_eq!(
            error.storage_error().ok_or("storage error")?.failure(),
            Failure::Rejected(Rejection::IsDirectory)
        );
        assert!(matches!(
            files.create_dir(CreateDirRequest::new(path.as_str(), ah_vfs_storage::CreateDirectoryMode::Recursive)?).await,
            Err(EntryError::MountBoundary(boundary)) if boundary == path
        ));
    }

    mounts.disable(mounted.id).await?;
    // A disabled submount still creates and shadows its synthetic ancestor.
    let page = files.list_entries(&ancestor, DirQuery::default()).await?;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].path, nested);
    assert_eq!(
        page.entries[0].origin,
        EntryOrigin::MountPoint { enabled: false }
    );
    assert!(matches!(files.stat_entry(&nested).await,
        Err(EntryError::Table(ah_vfs::domain::namespace::MountTableError::Disabled(id))) if id == mounted.id
    ));
    let error = files
        .read_file(ReadFileRequest {
            path: nested,
            range: ah_vfs_storage::ReadRange::new(0, None)?,
        })
        .await
        .err()
        .ok_or("disabled mount")?;
    assert!(matches!(error,
        EntryError::Table(ah_vfs::domain::namespace::MountTableError::Disabled(id)) if id == mounted.id
    ));
    Ok(())
}
