use ah_vfs::application::mount_runtime::MountRuntime;
mod common;

use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        driver_info::DriverInfo,
        dto::*,
        error::EntryError,
        port::{DriverFuture, Storage, StorageFileReader, StorageFileWriter, StorageFuture},
        service::{EntryService, MountService},
    },
    domain::{
        DriverInstance, DriverKey, InstanceId,
        namespace::{ChildMountPolicy, VirtualPath},
        storage::{
            StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageEntry,
            StorageEntryKind, StorageError, StorageErrorKind, StorageMoveEntryRequest, StoragePath,
            StorageReadFileRequest, StorageRemoveEntryRequest, StorageStatEntryRequest,
            StorageWriteFileRequest,
        },
    },
    infrastructure::mount_repository::SqliteMountRepository,
};

use std::{error::Error, num::NonZeroUsize, sync::Arc};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy)]
enum Response {
    Normal,
    WrongChild,
    StuckCursor,
}

struct TestDriver {
    info: DriverInfo,
    response: Response,
}
struct TestStorage {
    capabilities: ah_vfs::domain::storage::StorageCapabilities,
    instance: InstanceId,
    response: Response,
}

fn unsupported<T: Send + 'static>() -> StorageFuture<'static, T> {
    Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation)) })
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
                capabilities: self.info.capabilities(),
                instance,
                response: self.response,
            }) as Arc<dyn Storage>)
        })
    }
}

impl Storage for TestStorage {
    fn capabilities(&self) -> ah_vfs::domain::storage::StorageCapabilities {
        self.capabilities
    }
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn stat_entry(&self, request: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            Ok(StorageEntry {
                kind: if request.path.is_root() {
                    StorageEntryKind::Dir
                } else {
                    StorageEntryKind::File
                },
                path: request.path,
                size: Some(if self.instance.as_str() == "leaf" {
                    42
                } else {
                    7
                }),
                modified: None,
            })
        })
    }
    fn list_entries(&self, query: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        Box::pin(async move {
            let (names, next) = match self.response {
                Response::WrongChild => (vec!["nested/file"], None),
                Response::StuckCursor => (vec![], Some("same")),
                Response::Normal if query.after.is_none() => (vec!["a", "docs"], Some("docs")),
                Response::Normal => (vec!["z"], None),
            };
            Ok(StorageDirPage {
                entries: names
                    .into_iter()
                    .map(|name| {
                        Ok(StorageEntry {
                            path: StoragePath::parse(name)?,
                            kind: StorageEntryKind::File,
                            size: Some(1),
                            modified: None,
                        })
                    })
                    .collect::<Result<_, StorageError>>()?,
                next_after: next.map(str::to_owned),
            })
        })
    }
    fn read_file(
        &self,
        _: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        unsupported()
    }
    fn write_file(
        &self,
        _: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        unsupported()
    }
    fn create_dir(&self, _: StorageCreateDirRequest) -> StorageFuture<'_, StorageEntry> {
        unsupported()
    }
    fn remove_entry(&self, _: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        unsupported()
    }
    fn move_entry(&self, _: StorageMoveEntryRequest) -> StorageFuture<'_, StorageEntry> {
        unsupported()
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
                    ah_vfs::domain::storage::StorageCapabilities::builder()
                        .stat_entry(true)
                        .list_entries(true)
                        .build()?,
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
    assert_eq!(entry.size, Some(42));
    assert_eq!(
        files
            .stat_entry(&VirtualPath::parse("/docs-old")?)
            .await?
            .size,
        Some(7)
    );
    Ok(())
}

#[tokio::test]
async fn namespace_pagination_merges_every_backend_page_before_slicing() -> TestResult {
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
    let page = files
        .list_entries(
            &VirtualPath::root(),
            DirQuery {
                after: Some("a".into()),
                limit: NonZeroUsize::new(2).ok_or("zero limit")?,
            },
        )
        .await?;
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        ["/docs", "/z"]
    );
    assert_eq!(
        page.entries[0].origin,
        EntryOrigin::MountPoint { enabled: true }
    );
    assert!(page.next_after.is_none());
    Ok(())
}

#[tokio::test]
async fn malformed_backend_children_and_nonadvancing_cursors_are_rejected() -> TestResult {
    for response in [Response::WrongChild, Response::StuckCursor] {
        let (files, _) = services(response).await?;
        assert!(matches!(
            files
                .list_entries(&VirtualPath::root(), Default::default())
                .await,
            Err(EntryError::InvalidStorageResponse(_))
        ));
    }
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
    assert_eq!(root.size, Some(7));
    let synthetic = files.stat_entry(&ancestor).await?;
    assert_eq!(synthetic.origin, EntryOrigin::SyntheticDir);
    assert_eq!(synthetic.kind, StorageEntryKind::Dir);
    let leaf = files.stat_entry(&nested).await?;
    assert_eq!(leaf.origin, EntryOrigin::MountPoint { enabled: true });
    assert_eq!(leaf.size, Some(42));

    for path in [VirtualPath::root(), ancestor.clone(), nested.clone()] {
        let error = files
            .read_file(ReadFileRequest {
                path: path.clone(),
                offset: 0,
                length: None,
            })
            .await
            .err()
            .ok_or("namespace dir is not a file")?;
        assert_eq!(
            error.storage_error().ok_or("storage error")?.kind(),
            StorageErrorKind::IsDir
        );
        assert!(matches!(
            files.create_dir(CreateDirRequest { path: path.clone(), recursive: true }).await,
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
            offset: 0,
            length: None,
        })
        .await
        .err()
        .ok_or("disabled mount")?;
    assert!(matches!(error,
        EntryError::Table(ah_vfs::domain::namespace::MountTableError::Disabled(id)) if id == mounted.id
    ));
    Ok(())
}
