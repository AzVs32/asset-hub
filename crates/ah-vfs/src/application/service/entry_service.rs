//! Driver-independent entry access, namespace overlays and mount-boundary protection.

use crate::{
    application::{
        dto::{
            CreateDirRequest, DirPage, DirQuery, EntryOrigin, EntryView, MoveEntryRequest,
            ReadFileRequest, RemoveEntryRequest, WriteFileRequest,
        },
        error::EntryError,
        mount_runtime::MountRuntime,
        port::Storage,
        session::{FileReadHandle, FileWriteHandle},
    },
    domain::{
        namespace::{Mount, MountTable, VirtualPath},
        storage::{
            EntryPermissions, FileReadSupport, MAX_DIR_PAGE_LIMIT, StorageCapabilities,
            StorageCreateDirRequest, StorageDirQuery, StorageEntryKind, StorageError,
            StorageErrorKind, StorageMoveEntryRequest, StoragePath, StorageReadFileRequest,
            StorageRemoveEntryRequest, StorageStatEntryRequest, StorageWriteFileRequest, XattrName,
            XattrWriteMode, validate_dir_page_limit,
        },
    },
};

use std::{collections::BTreeMap, num::NonZeroUsize, sync::Arc};

/// Resolve paths through a shared runtime snapshot and reuse validated storage.
/// Concurrent namespace changes are not serialized with I/O. Open streams retain
/// their original backend even if a mount changes later.
pub struct EntryService {
    runtime: Arc<MountRuntime>,
}

enum NamespaceEntry<'a> {
    Backend,
    MountPoint(&'a Mount),
    SyntheticDir,
}

impl EntryService {
    pub fn new(runtime: Arc<MountRuntime>) -> Self {
        Self { runtime }
    }

    async fn open(
        &self,
        table: &MountTable,
        path: &VirtualPath,
    ) -> Result<(StoragePath, Arc<dyn Storage>), EntryError> {
        let resolution = table.resolve(path)?;
        let reference = resolution.mount().driver_instance();
        let driver = self
            .runtime
            .drivers
            .get(reference.driver_key())
            .ok_or_else(|| EntryError::DriverNotRegistered(reference.driver_key().clone()))?;
        let relative = resolution.storage_path().map_err(StorageError::from)?;
        let storage = self.runtime.storage(reference, driver.as_ref()).await?;
        Ok((relative, storage))
    }

    fn require(supported: bool) -> Result<(), EntryError> {
        if supported {
            Ok(())
        } else {
            Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into())
        }
    }

    /// Effective backing-instance capabilities, not path authorization. Namespace
    /// protection and synthetic-dir behavior are checked by each operation.
    pub async fn capabilities(
        &self,
        path: &VirtualPath,
    ) -> Result<StorageCapabilities, EntryError> {
        let table = self.runtime.table().await?;
        let (_, storage) = self.open(&table, path).await?;
        Ok(storage.capabilities())
    }

    fn classify<'a>(table: &'a MountTable, path: &VirtualPath) -> NamespaceEntry<'a> {
        if let Some(mount) = table.get_by_path(path) {
            NamespaceEntry::MountPoint(mount)
        } else if table
            .list()
            .iter()
            .any(|mount| mount.virtual_path() != path && path.contains(mount.virtual_path()))
        {
            NamespaceEntry::SyntheticDir
        } else {
            NamespaceEntry::Backend
        }
    }

    fn protect(table: &MountTable, path: &VirtualPath) -> Result<(), EntryError> {
        table.resolve(path)?;
        if !matches!(Self::classify(table, path), NamespaceEntry::Backend) {
            return Err(EntryError::MountBoundary(path.clone()));
        }
        Ok(())
    }

    /// Retrieve backend metadata, or a synthetic directory leading to submounts.
    pub async fn stat_entry(&self, path: &VirtualPath) -> Result<EntryView, EntryError> {
        let table = self.runtime.table().await?;
        table.resolve(path)?;
        let namespace = Self::classify(&table, path);
        if matches!(namespace, NamespaceEntry::SyntheticDir) {
            return Ok(EntryView::dir(path.clone(), EntryOrigin::SyntheticDir));
        }
        let (relative, storage) = self.open(&table, path).await?;
        Self::require(storage.capabilities().stat_entry)?;
        let entry = storage
            .stat_entry(StorageStatEntryRequest { path: relative })
            .await?;
        let mut view = EntryView::from_storage(path.clone(), entry);
        if let NamespaceEntry::MountPoint(mount) = namespace {
            view.origin = EntryOrigin::MountPoint {
                enabled: mount.enabled(),
            };
        }
        Ok(view)
    }

    /// Merge backend children with direct mounts and synthetic ancestors.
    /// Mount entries shadow backend names, including disabled mounts. Pagination
    /// is live and scans backend pages before slicing the merged result.
    pub async fn list_entries(
        &self,
        path: &VirtualPath,
        query: DirQuery,
    ) -> Result<DirPage, EntryError> {
        validate_dir_page_limit(query.limit)?;
        if let Some(after) = &query.after {
            path.join(after)?;
        }
        let table = self.runtime.table().await?;
        let synthetic = matches!(Self::classify(&table, path), NamespaceEntry::SyntheticDir);
        let (relative, storage) = self.open(&table, path).await?;
        Self::require(storage.capabilities().list_entries)?;
        if synthetic {
            Self::require(storage.capabilities().stat_entry)?;
        }
        let mut entries = BTreeMap::new();
        let mut after = None;
        // A namespace directory shadows a same-name backend file or symlink.
        let scan_backend = if synthetic {
            match storage
                .stat_entry(StorageStatEntryRequest {
                    path: relative.clone(),
                })
                .await
            {
                Ok(entry) => entry.kind == StorageEntryKind::Dir,
                Err(error)
                    if matches!(
                        error.kind(),
                        StorageErrorKind::NotFound | StorageErrorKind::NotDir
                    ) =>
                {
                    false
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            true
        };
        if scan_backend {
            loop {
                let page = storage
                    .list_entries(StorageDirQuery {
                        path: relative.clone(),
                        after: after.clone(),
                        limit: MAX_DIR_PAGE_LIMIT,
                    })
                    .await;
                let page = match page {
                    Ok(page) => page,
                    Err(error)
                        if synthetic
                            && matches!(
                                error.kind(),
                                StorageErrorKind::NotFound | StorageErrorKind::NotDir
                            ) =>
                    {
                        break;
                    }
                    Err(error) => return Err(error.into()),
                };
                for entry in page.entries {
                    if entry.path.parent().as_ref() != Some(&relative) {
                        return Err(EntryError::InvalidStorageResponse(
                            "entry is not an immediate child",
                        ));
                    }
                    let name = entry
                        .path
                        .name()
                        .ok_or(EntryError::InvalidStorageResponse("child has no name"))?
                        .to_owned();
                    entries.insert(
                        name.clone(),
                        EntryView::from_storage(path.join(&name)?, entry),
                    );
                }
                match page.next_after {
                    Some(next) if after.as_ref().is_none_or(|previous| next > *previous) => {
                        after = Some(next)
                    }
                    Some(_) => {
                        return Err(EntryError::InvalidStorageResponse(
                            "directory cursor did not advance",
                        ));
                    }
                    None => break,
                }
            }
        }
        for mount in table.list() {
            let Some(suffix) = mount.virtual_path().strip_prefix(path) else {
                continue;
            };
            let Some(name) = suffix.split('/').find(|name| !name.is_empty()) else {
                continue;
            };
            let child = path.join(name)?;
            let origin = table
                .get_by_path(&child)
                .map_or(EntryOrigin::SyntheticDir, |direct| {
                    EntryOrigin::MountPoint {
                        enabled: direct.enabled(),
                    }
                });
            entries.insert(name.to_owned(), EntryView::dir(child, origin));
        }
        let mut entries: Vec<_> = entries
            .into_iter()
            .filter(|(name, _)| query.after.as_ref().is_none_or(|after| name > after))
            .take(query.limit.get() + 1)
            .map(|(_, entry)| entry)
            .collect();
        let more = entries.len() > query.limit.get();
        entries.truncate(query.limit.get());
        let next_after = more
            .then(|| {
                entries
                    .last()
                    .and_then(|entry| entry.path.name().map(str::to_owned))
            })
            .flatten();
        Ok(DirPage {
            entries,
            next_after,
        })
    }

    /// Open a raw byte range; namespace directories cannot be read as files.
    pub async fn read_file(&self, request: ReadFileRequest) -> Result<FileReadHandle, EntryError> {
        let table = self.runtime.table().await?;
        table.resolve(&request.path)?;
        if !matches!(
            Self::classify(&table, &request.path),
            NamespaceEntry::Backend
        ) {
            return Err(StorageError::new(StorageErrorKind::IsDir).into());
        }
        let (relative, storage) = self.open(&table, &request.path).await?;
        let support = storage.capabilities().read_file;
        Self::require(support != FileReadSupport::None)?;
        Self::require(
            (request.offset == 0 && request.length.is_none()) || support == FileReadSupport::Range,
        )?;
        let reader = storage
            .read_file(StorageReadFileRequest {
                path: relative,
                offset: request.offset,
                length: request.length,
            })
            .await?;
        Ok(FileReadHandle::new(reader, storage.chunk_size()))
    }

    /// Stage a complete-file write; changes publish only on the returned handle's commit.
    pub async fn write_file(
        &self,
        request: WriteFileRequest,
    ) -> Result<FileWriteHandle, EntryError> {
        let table = self.runtime.table().await?;
        Self::protect(&table, &request.path)?;
        let (relative, storage) = self.open(&table, &request.path).await?;
        let support = storage.capabilities().write_file;
        Self::require(match request.mode {
            crate::domain::storage::StorageWriteFileMode::CreateNew => support.create_new,
            crate::domain::storage::StorageWriteFileMode::Replace => support.replace,
        })?;
        Self::require(!request.require_atomic_publish || support.atomic_publish)?;
        let writer = storage
            .write_file(StorageWriteFileRequest {
                require_atomic_publish: request.require_atomic_publish,
                path: relative,
                mode: request.mode,
            })
            .await?;
        Ok(FileWriteHandle::new(
            writer,
            storage.chunk_size(),
            request.path,
        ))
    }

    /// Create backend directories without modifying namespace-owned directories.
    /// Mount points and their ancestors remain protected for recursive requests.
    pub async fn create_dir(&self, request: CreateDirRequest) -> Result<EntryView, EntryError> {
        let table = self.runtime.table().await?;
        Self::protect(&table, &request.path)?;
        let (relative, storage) = self.open(&table, &request.path).await?;
        Self::require(storage.capabilities().create_dir)?;
        let entry = storage
            .create_dir(StorageCreateDirRequest {
                path: relative,
                recursive: request.recursive,
            })
            .await?;
        Ok(EntryView::from_storage(request.path, entry))
    }

    /// Remove a backend file, link or empty directory, preserving all mount boundaries.
    pub async fn remove_entry(&self, request: RemoveEntryRequest) -> Result<(), EntryError> {
        let table = self.runtime.table().await?;
        Self::protect(&table, &request.path)?;
        let (relative, storage) = self.open(&table, &request.path).await?;
        Self::require(storage.capabilities().remove_entry)?;
        Ok(storage
            .remove_entry(StorageRemoveEntryRequest { path: relative })
            .await?)
    }

    /// Rename within one mount. Cross-mount moves and changes containing mounts are rejected.
    pub async fn move_entry(&self, request: MoveEntryRequest) -> Result<EntryView, EntryError> {
        let table = self.runtime.table().await?;
        Self::protect(&table, &request.source)?;
        Self::protect(&table, &request.destination)?;
        let source = table.resolve(&request.source)?;
        let destination = table.resolve(&request.destination)?;
        if source.mount().id() != destination.mount().id() {
            return Err(EntryError::CrossMountMove);
        }
        let destination = destination.storage_path().map_err(StorageError::from)?;
        let (source, storage) = self.open(&table, &request.source).await?;
        let support = storage.capabilities().rename;
        Self::require(if request.replace {
            support.replace
        } else {
            support.no_replace
        })?;
        let entry = storage
            .move_entry(StorageMoveEntryRequest {
                source,
                destination,
                replace: request.replace,
            })
            .await?;
        Ok(EntryView::from_storage(request.destination, entry))
    }
    async fn attribute_storage(
        &self,
        path: &VirtualPath,
        write: bool,
    ) -> Result<(StoragePath, Arc<dyn Storage>), EntryError> {
        let table = self.runtime.table().await?;
        if write {
            Self::protect(&table, path)?;
        }
        if matches!(Self::classify(&table, path), NamespaceEntry::SyntheticDir) {
            return Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into());
        }
        let (relative, storage) = self.open(&table, path).await?;
        Ok((relative, storage))
    }

    /// Read POSIX mode bits. Synthetic dirs have no backend attributes.
    pub async fn get_permissions(
        &self,
        path: &VirtualPath,
    ) -> Result<EntryPermissions, EntryError> {
        let (relative, storage) = self.attribute_storage(path, false).await?;
        Self::require(storage.capabilities().permissions.can_read())?;
        let extension = storage
            .permissions()
            .ok_or(EntryError::InvalidStorageResponse(
                "missing permissions extension",
            ))?;
        Ok(extension.get_permissions(relative).await?)
    }

    /// Change mode bits without bypassing mount/ancestor protection.
    pub async fn set_permissions(
        &self,
        path: &VirtualPath,
        permissions: EntryPermissions,
    ) -> Result<(), EntryError> {
        let (relative, storage) = self.attribute_storage(path, true).await?;
        Self::require(storage.capabilities().permissions.can_write())?;
        let extension = storage
            .permissions()
            .ok_or(EntryError::InvalidStorageResponse(
                "missing permissions extension",
            ))?;
        Ok(extension.set_permissions(relative, permissions).await?)
    }

    /// Return sorted opaque attribute names within the supplied byte budget.
    pub async fn list_xattrs(
        &self,
        path: &VirtualPath,
        max_bytes: NonZeroUsize,
    ) -> Result<Vec<XattrName>, EntryError> {
        let (relative, storage) = self.attribute_storage(path, false).await?;
        Self::require(storage.capabilities().xattrs.can_read())?;
        let extension = storage.xattrs().ok_or(EntryError::InvalidStorageResponse(
            "missing xattr extension",
        ))?;
        Ok(extension.list_xattrs(relative, max_bytes).await?)
    }

    /// Return a complete opaque attribute value, never a truncated prefix.
    pub async fn get_xattr(
        &self,
        path: &VirtualPath,
        name: XattrName,
        max_bytes: NonZeroUsize,
    ) -> Result<Vec<u8>, EntryError> {
        let (relative, storage) = self.attribute_storage(path, false).await?;
        Self::require(storage.capabilities().xattrs.can_read())?;
        let extension = storage.xattrs().ok_or(EntryError::InvalidStorageResponse(
            "missing xattr extension",
        ))?;
        Ok(extension.get_xattr(relative, name, max_bytes).await?)
    }

    /// Atomically apply the attribute existence policy; preserve namespace boundaries.
    pub async fn set_xattr(
        &self,
        path: &VirtualPath,
        name: XattrName,
        value: Vec<u8>,
        mode: XattrWriteMode,
    ) -> Result<(), EntryError> {
        let (relative, storage) = self.attribute_storage(path, true).await?;
        Self::require(storage.capabilities().xattrs.can_write())?;
        let extension = storage.xattrs().ok_or(EntryError::InvalidStorageResponse(
            "missing xattr extension",
        ))?;
        Ok(extension.set_xattr(relative, name, value, mode).await?)
    }

    /// Remove one attribute, returning AttributeNotFound for an absent key.
    pub async fn remove_xattr(
        &self,
        path: &VirtualPath,
        name: XattrName,
    ) -> Result<(), EntryError> {
        let (relative, storage) = self.attribute_storage(path, true).await?;
        Self::require(storage.capabilities().xattrs.can_write())?;
        let extension = storage.xattrs().ok_or(EntryError::InvalidStorageResponse(
            "missing xattr extension",
        ))?;
        Ok(extension.remove_xattr(relative, name).await?)
    }
}
