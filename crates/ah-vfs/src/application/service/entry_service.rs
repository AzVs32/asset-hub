//! Driver-independent entry access, namespace overlays and mount-boundary protection.

use crate::application::dto::{
    CreateDirRequest, DirPage, DirQuery, EntryOrigin, EntryView, MoveEntryRequest, ReadFileRequest,
    RemoveEntryRequest, VfsDirectoryCursor, WriteFileRequest,
};
use crate::application::error::EntryError;
use crate::application::mount_runtime::MountRuntime;
use crate::application::port::{LookupContext, Storage};
use crate::application::session::{FileReadHandle, FileWriteHandle};
use crate::domain::namespace::{Mount, MountTable, VirtualPath};
use ah_vfs_storage::{AttributeAction, AttributeRequest, AttributeResult, EntryType, StoragePath};
use ah_vfs_storage::{BackendFailure, DirectoryCursor, ListRequest, ListTarget};
use ah_vfs_storage::{
    Capabilities, LinkTarget, ReadRequest, RemoveRequest, RenameRequest,
    StatRequest, SymlinkAction, WriteRequest,
};
use ah_vfs_storage::{Failure, Operation, Rejection, StorageError};

use std::{collections::BTreeMap, sync::Arc};

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
    ) -> Result<(StoragePath, Arc<dyn Storage>, Capabilities), EntryError> {
        let resolution = table.resolve(path)?;
        let reference = resolution.mount().driver_instance();
        let driver = self
            .runtime
            .drivers
            .get(reference.driver_key())
            .ok_or_else(|| EntryError::DriverNotRegistered(reference.driver_key().clone()))?;
        let relative = resolution.storage_path()?;
        let storage = self.runtime.storage(reference, driver.as_ref()).await?;
        Ok((relative, storage, driver.info().capabilities()))
    }

    fn require(operation: Operation, supported: bool) -> Result<(), EntryError> {
        if supported {
            Ok(())
        } else {
            Err(StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::Unsupported),
                None,
            )
            .into())
        }
    }

    /// Driver-wide capabilities for the resolved mount. No instance is opened and
    /// no backend path is probed. This does not report authorization or availability.
    pub async fn capabilities(&self, path: &VirtualPath) -> Result<Capabilities, EntryError> {
        let table = self.runtime.table().await?;
        let resolution = table.resolve(path)?;
        let key = resolution.mount().driver_instance().driver_key();
        let driver = self
            .runtime
            .drivers
            .get(key)
            .ok_or_else(|| EntryError::DriverNotRegistered(key.clone()))?;
        Ok(driver.info().capabilities())
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
    /// Resolve the virtual path once. Any intermediate link traversal stays
    /// inside the selected instance, and the final link is preserved.
    pub async fn stat_entry(&self, path: &VirtualPath) -> Result<EntryView, EntryError> {
        let table = self.runtime.table().await?;
        table.resolve(path)?;
        let namespace = Self::classify(&table, path);
        if matches!(namespace, NamespaceEntry::SyntheticDir) {
            return Ok(EntryView::dir(path.clone(), EntryOrigin::SyntheticDir));
        }
        let (relative, storage, capabilities) = self.open(&table, path).await?;
        Self::require(Operation::Stat, capabilities.stat())?;
        let entry = storage
            .stat_entry(
                StatRequest::new(relative.to_rooted_expr(), SymlinkAction::Preserve),
                LookupContext::default(),
            )
            .await?;
        let mut view = EntryView::from_storage(path.clone(), entry);
        if let NamespaceEntry::MountPoint(mount) = namespace {
            view.origin = EntryOrigin::MountPoint {
                enabled: mount.enabled(),
            };
        }
        Ok(view)
    }

    /// Read a symbolic link in the resolved instance. The stored bytes are returned
    /// as-is, including for dangling links.
    pub async fn read_link(&self, path: &VirtualPath) -> Result<LinkTarget, EntryError> {
        let table = self.runtime.table().await?;
        table.resolve(path)?;
        if !matches!(Self::classify(&table, path), NamespaceEntry::Backend) {
            return Err(StorageError::without_effects(
                Operation::ReadLink,
                Failure::Rejected(Rejection::NotSymlink),
                None,
            )
            .into());
        }
        let (relative, storage, capabilities) = self.open(&table, path).await?;
        Self::require(Operation::ReadLink, capabilities.read_link())?;
        Ok(storage
            .read_link(
                relative.to_rooted_expr(),
                LookupContext::default(),
            )
            .await?)
    }

    /// Merge live backend pages with the current virtual namespace.
    /// A continuation keeps the mount snapshot and the selected directory.
    pub async fn list_entries(
        &self,
        path: &VirtualPath,
        query: DirQuery,
    ) -> Result<DirPage, EntryError> {
        let stale = || {
            StorageError::without_effects(
                Operation::List,
                Failure::Backend(BackendFailure::Stale),
                None,
            )
        };
        let table = self.runtime.table().await?;
        let cursor = query.cursor;
        if cursor
            .as_ref()
            .is_some_and(|cursor| &cursor.path != path || !Arc::ptr_eq(&cursor.mounts, &table))
        {
            return Err(stale().into());
        }
        let (storage, directory) = match &cursor {
            Some(cursor) => {
                if cursor.storage.is_stale() {
                    return Err(stale().into());
                }
                (cursor.storage.clone(), cursor.directory.clone())
            }
            None => {
                let (relative, storage, capabilities) = self.open(&table, path).await?;
                Self::require(Operation::List, capabilities.list())?;
                let context = LookupContext::default();
                let request =
                    ListRequest::new(ListTarget::Path(relative.to_rooted_expr()), query.size);
                let directory =
                    if matches!(Self::classify(&table, path), NamespaceEntry::SyntheticDir) {
                        Self::require(Operation::List, capabilities.stat())?;
                        match storage
                            .stat_entry(
                                StatRequest::new(
                                    relative.to_rooted_expr(),
                                    SymlinkAction::Preserve,
                                ),
                                context.clone(),
                            )
                            .await
                        {
                            Ok(entry) if entry.entry_type() == EntryType::Directory => {
                                let selection = storage.select_directory(&request, context).await?;
                                if selection.address() != &relative {
                                    return Err(StorageError::without_effects(
                                        Operation::List,
                                        Failure::Rejected(Rejection::Conflict),
                                        None,
                                    )
                                    .into());
                                }
                                Some(selection)
                            }
                            Ok(_) => None,
                            Err(error)
                                if matches!(
                                    error.failure(),
                                    Failure::Rejected(
                                        Rejection::NotFound | Rejection::NotDirectory
                                    )
                                ) =>
                            {
                                None
                            }
                            Err(error) => return Err(error.into()),
                        }
                    } else {
                        Some(storage.select_directory(&request, context).await?)
                    };
                (storage, directory)
            }
        };

        let after = cursor.as_ref().map(|cursor| cursor.after.clone());
        let limit = query.size.get();
        let mut candidates = BTreeMap::<String, (EntryView, Option<DirectoryCursor>)>::new();
        for mount in table.iter() {
            let Some(suffix) = mount.virtual_path().strip_prefix(path) else {
                continue;
            };
            let Some(name) = suffix.split('/').find(|name| !name.is_empty()) else {
                continue;
            };
            if after.as_deref().is_some_and(|after| name <= after) {
                continue;
            }
            let child = path.join(name)?;
            let origin = table
                .get_by_path(&child)
                .map_or(EntryOrigin::SyntheticDir, |mount| EntryOrigin::MountPoint {
                    enabled: mount.enabled(),
                });
            candidates.insert(name.to_owned(), (EntryView::dir(child, origin), None));
            if candidates.len() > limit + 1 {
                candidates.pop_last();
            }
        }

        // Keep only one page of merged candidates plus a lookahead. A shadowed
        // backend name still advances its instance cursor when emitted.
        let mut backend = cursor.and_then(|cursor| cursor.backend);
        if let Some(selection) = &directory {
            let mut scan = backend.clone();
            'pages: loop {
                let target = scan.map_or_else(
                    || ListTarget::Path(selection.address().to_rooted_expr()),
                    ListTarget::Continue,
                );
                let request = ListRequest::new(target, query.size);
                let bound = request
                    .bind_selection(selection.clone())
                    .map_err(|_| stale())?;
                let page = storage.list_page(&bound).await?;
                if page.selection() != selection
                    || page.entries().len() > limit
                    || page.entries().first().is_some_and(|entry| {
                        bound.after().is_some_and(|after| {
                            entry.path().name().is_none_or(|name| &name <= after)
                        })
                    })
                {
                    return Err(EntryError::InvalidStorageResponse(
                        "directory page disagrees with its binding",
                    ));
                }
                for (index, entry) in page.entries().iter().enumerate() {
                    let checkpoint = page
                        .checkpoint(index)
                        .ok_or(EntryError::InvalidStorageResponse("child has no name"))?;
                    let name = checkpoint.after().as_str();
                    if after.as_deref().is_some_and(|after| name <= after) {
                        backend = Some(checkpoint);
                        continue;
                    }
                    if candidates.len() > limit
                        && candidates
                            .last_key_value()
                            .is_some_and(|(last, _)| name > last.as_str())
                    {
                        break 'pages;
                    }
                    if let Some((_, position)) = candidates.get_mut(name) {
                        *position = Some(checkpoint);
                    } else {
                        let virtual_path = path.join(name)?;
                        candidates.insert(
                            name.to_owned(),
                            (
                                EntryView::from_storage(virtual_path, entry.clone()),
                                Some(checkpoint),
                            ),
                        );
                        if candidates.len() > limit + 1 {
                            candidates.pop_last();
                        }
                    }
                }
                let Some(next) = page.continuation() else {
                    break;
                };
                scan = Some(next);
            }
        }

        let more = candidates.len() > limit;
        let mut entries = Vec::with_capacity(limit.min(candidates.len()));
        let mut last = None;
        for (name, (entry, position)) in candidates.into_iter().take(limit) {
            if position.is_some() {
                backend = position;
            }
            last = Some(name);
            entries.push(entry);
        }
        let continuation = if more {
            last.map(|after| VfsDirectoryCursor {
                path: path.clone(),
                mounts: table,
                storage,
                after,
                directory,
                backend,
            })
        } else {
            None
        };
        Ok(DirPage {
            entries,
            continuation,
        })
    }

    /// Open a raw byte range; namespace directories cannot be read as files.
    pub async fn read_file(&self, request: ReadFileRequest) -> Result<FileReadHandle, EntryError> {
        let table = self.runtime.table().await?;
        let resolution = table.resolve(&request.path)?;
        if !matches!(
            Self::classify(&table, &request.path),
            NamespaceEntry::Backend
        ) {
            return Err(StorageError::without_effects(
                Operation::Read,
                Failure::Rejected(Rejection::IsDirectory),
                None,
            )
            .into());
        }
        let reference = resolution.mount().driver_instance();
        let driver = self
            .runtime
            .drivers
            .get(reference.driver_key())
            .ok_or_else(|| EntryError::DriverNotRegistered(reference.driver_key().clone()))?;
        let request = ReadRequest::new(resolution.storage_path()?.to_rooted_expr(), request.range);
        driver
            .info()
            .capabilities()
            .require_read(&request)
            .map_err(|reason| {
                StorageError::without_effects(Operation::Read, Failure::Rejected(reason), None)
            })?;
        let storage = self.runtime.storage(reference, driver.as_ref()).await?;
        let reader = storage.read_file(request, LookupContext::default()).await?;
        Ok(FileReadHandle::new(reader, storage.chunk_size()))
    }

    /// Stage a complete-file write; changes publish only on the returned handle's commit.
    pub async fn write_file(
        &self,
        request: WriteFileRequest,
    ) -> Result<FileWriteHandle, EntryError> {
        let table = self.runtime.table().await?;
        Self::protect(&table, &request.path)?;
        let resolution = table.resolve(&request.path)?;
        let reference = resolution.mount().driver_instance();
        let driver = self
            .runtime
            .drivers
            .get(reference.driver_key())
            .ok_or_else(|| EntryError::DriverNotRegistered(reference.driver_key().clone()))?;
        let operation = WriteRequest::new(
            resolution.storage_path()?.to_rooted_expr(),
            request.mode,
            request.publication,
        );
        driver
            .info()
            .capabilities()
            .require_write(&operation)
            .map_err(|reason| {
                StorageError::without_effects(Operation::Write, Failure::Rejected(reason), None)
            })?;
        let storage = self.runtime.storage(reference, driver.as_ref()).await?;
        let writer = storage
            .write_file(operation, LookupContext::default())
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
        let resolution = table.resolve(&request.path)?;
        let reference = resolution.mount().driver_instance();
        let driver = self
            .runtime
            .drivers
            .get(reference.driver_key())
            .ok_or_else(|| EntryError::DriverNotRegistered(reference.driver_key().clone()))?;
        Self::require(
            Operation::CreateDirectory,
            driver.info().capabilities().directories().can_create(),
        )?;
        let mut operand = resolution.storage_path()?.to_rooted_expr();
        if request.trailing_slash {
            operand = operand.with_trailing_slash();
        }
        let operation = ah_vfs_storage::CreateDirectoryRequest::new(operand, request.mode);
        let storage = self.runtime.storage(reference, driver.as_ref()).await?;
        let entry = storage
            .create_dir(operation, LookupContext::default())
            .await?;
        Ok(EntryView::from_storage(request.path, entry))
    }

    /// Remove one backend entry while preserving VFS mount boundaries.
    pub async fn remove_entry(&self, request: RemoveEntryRequest) -> Result<(), EntryError> {
        let table = self.runtime.table().await?;
        Self::protect(&table, &request.path)?;
        let (relative, storage, capabilities) = self.open(&table, &request.path).await?;
        Self::require(Operation::Remove, capabilities.remove())?;
        let operand = mutation_operand(relative, request.trailing_slash);
        let remove = RemoveRequest::new(operand, request.kind)
            .map_err(|_| protected_root(Operation::Remove))?;
        Ok(storage
            .remove_entry(remove, LookupContext::default())
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
        let destination = mutation_operand(destination.storage_path()?, request.destination_slash);
        let (source, storage, capabilities) = self.open(&table, &request.source).await?;
        Self::require(
            Operation::Rename,
            capabilities.rename().supports(request.mode),
        )?;
        let source = mutation_operand(source, request.source_slash);
        let rename = RenameRequest::new(source, destination, request.mode)
            .map_err(|_| protected_root(Operation::Rename))?;
        let entry = storage.move_entry(rename, LookupContext::default()).await?;
        Ok(EntryView::from_storage(request.destination, entry))
    }

    /// Execute an attribute action on an absolute VFS operand.
    pub async fn attribute(
        &self,
        operand: &str,
        action: AttributeAction,
    ) -> Result<AttributeResult, EntryError> {
        let path = VirtualPath::parse(operand)?;
        let operation = action.operation();
        let table = self.runtime.table().await?;
        if operation.may_mutate() {
            Self::protect(&table, &path)?;
        }
        if matches!(Self::classify(&table, &path), NamespaceEntry::SyntheticDir) {
            Self::require(operation, false)?;
        }
        let resolution = table.resolve(&path)?;
        let reference = resolution.mount().driver_instance();
        let driver = self
            .runtime
            .drivers
            .get(reference.driver_key())
            .ok_or_else(|| EntryError::DriverNotRegistered(reference.driver_key().clone()))?;
        Self::require(operation, driver.info().capabilities().supports(operation))?;
        let operand = mutation_operand(resolution.storage_path()?, operand.ends_with('/'));
        let expected = action.expectation();
        let storage = self.runtime.storage(reference, driver.as_ref()).await?;
        let attributes = storage
            .attributes()
            .ok_or(EntryError::InvalidStorageResponse(
                "missing attribute interface",
            ))?;
        let result = attributes
            .execute(
                AttributeRequest::new(operand, action),
                LookupContext::default(),
            )
            .await?;
        expected.validate(&result).map_err(|reason| {
            StorageError::after_dispatch(operation, Failure::Contract(reason), None)
        })?;
        Ok(result)
    }
}

fn mutation_operand(path: StoragePath, trailing_slash: bool) -> ah_vfs_storage::PathExpr {
    let operand = path.to_rooted_expr();
    if trailing_slash {
        operand.with_trailing_slash()
    } else {
        operand
    }
}

fn protected_root(operation: Operation) -> StorageError {
    StorageError::without_effects(operation, Failure::Rejected(Rejection::RootProtected), None)
}
