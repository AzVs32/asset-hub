//! Directory-handle based local storage, with staged complete-file writes.

use ah_vfs::application::port::{DirectoryBindings, LookupContext};
use ah_vfs::application::port::{
    Storage, StorageAttributes, StorageFileReader, StorageFileWriter, StorageFuture,
};
use ah_vfs_storage::{
    BackendFailure, BoundListRequest, DirectoryPage, DirectorySelection, Failure, ListRequest,
    ListTarget, Operation, StorageError,
};
use ah_vfs_storage::{Entry, LinkTarget, ReadLinkRequest, ReadRequest, StatRequest, WriteRequest};

use cap_std::fs::Dir;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod attributes;
mod file_reader;
mod file_writer;
mod fs;
mod lookup;

const CHUNK_SIZE: usize = 64 * 1024;
// This namespace is inaccessible through Storage and omitted from listings.
const TEMP_PREFIX: &str = ".asset-hub-write-";

async fn blocking<T: Send + 'static>(
    context: Operation,
    may_publish: bool,
    operation: impl FnOnce() -> Result<T, StorageError> + Send + 'static,
) -> Result<T, StorageError> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            let kind = if error.is_cancelled() {
                Failure::Backend(BackendFailure::Cancelled)
            } else {
                Failure::Backend(BackendFailure::Other)
            };
            if may_publish {
                StorageError::after_dispatch(context, kind, Some(Box::new(error)))
            } else {
                StorageError::without_effects(context, kind, Some(Box::new(error)))
            }
        })?
}

/// An opened directory capability. Metadata operations apply their explicit lookup policy.
///
/// Directory handles pin the accessed directories even if paths are renamed.
/// Reads and writes retain their own resources independently of this object.
pub struct LinuxLocalStorage {
    root: PathBuf,
    dir: Arc<Dir>,
    directory_bindings: Arc<DirectoryBindings<Dir>>,
}

impl LinuxLocalStorage {
    pub(crate) async fn open(
        root: PathBuf,
    ) -> Result<Self, ah_vfs::application::driver_error::LifecycleError> {
        use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
        let path = root.clone();
        let dir = tokio::task::spawn_blocking(move || {
            Dir::open_ambient_dir(path, cap_std::ambient_authority())
        })
        .await
        .map_err(|source| LifecycleError::with_source(LifecycleFailure::Unavailable, source))?
        .map_err(|source| LifecycleError::with_source(LifecycleFailure::Backend, source))?;
        Ok(Self {
            root,
            dir: Arc::new(dir),
            directory_bindings: Arc::new(DirectoryBindings::default()),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Storage for LinuxLocalStorage {
    fn attributes(&self) -> Option<&dyn StorageAttributes> {
        Some(self)
    }

    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(CHUNK_SIZE).unwrap_or(NonZeroUsize::MIN)
    }

    fn stat_entry(&self, request: StatRequest, context: LookupContext) -> StorageFuture<'_, Entry> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Stat, false, move || {
            lookup::stat(&dir, request.lookup(), context)
        }))
    }

    fn read_link(
        &self,
        request: ReadLinkRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, LinkTarget> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::ReadLink, false, move || {
            lookup::read_link(&dir, request.lookup(), context)
        }))
    }

    fn select_directory<'a>(
        &'a self,
        request: &'a ListRequest,
        context: LookupContext,
    ) -> StorageFuture<'a, DirectorySelection> {
        let request = request.clone();
        let root = self.dir.clone();
        let bindings = self.directory_bindings.clone();
        Box::pin(blocking(Operation::List, false, move || {
            match request.target() {
                ListTarget::Path(_) => {
                    let (address, dir) = lookup::listing_directory(
                        &root,
                        request.lookup().ok_or_else(lookup::list_stale)?,
                        context,
                    )?;
                    bindings.bind(address, dir, lookup::same_directory)
                }
                ListTarget::Continue(cursor) => {
                    let selection = cursor.selection();
                    let dir = lookup::reopen_listing_directory(&root, selection)?;
                    bindings.verify(selection, &dir, lookup::same_directory)?;
                    Ok(selection.clone())
                }
            }
        }))
    }

    fn list_page<'a>(
        &'a self,
        bound: &'a BoundListRequest<'_>,
    ) -> StorageFuture<'a, DirectoryPage> {
        let request = bound.request().clone();
        let selection = bound.selection().clone();
        let root = self.dir.clone();
        let bindings = self.directory_bindings.clone();
        Box::pin(blocking(Operation::List, false, move || {
            let dir = lookup::reopen_listing_directory(&root, &selection)?;
            bindings.verify(&selection, &dir, lookup::same_directory)?;
            let bound = request
                .bind_selection(selection)
                .map_err(|_| lookup::list_stale())?;
            let page = fs::list_entries(&dir, &bound)?;
            let current = lookup::reopen_listing_directory(&root, bound.selection())?;
            if !lookup::same_directory(&dir, &current)? {
                return Err(lookup::list_stale());
            }
            Ok(page)
        }))
    }
    fn read_file(
        &self,
        request: ReadRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Read, false, move || {
            let file = lookup::open_reader(&dir, request.lookup(), context)?;
            Ok(
                Box::new(file_reader::LocalFileReader::open(file, request.range())?)
                    as Box<dyn StorageFileReader>,
            )
        }))
    }

    fn write_file(
        &self,
        request: WriteRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Write, false, move || {
            Ok(
                Box::new(file_writer::LocalFileWriter::open(dir, request, context)?)
                    as Box<dyn StorageFileWriter>,
            )
        }))
    }

    fn create_dir(
        &self,
        request: ah_vfs_storage::CreateDirectoryRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::CreateDirectory, true, move || {
            lookup::create_dir(&dir, &request, context)
        }))
    }

    fn remove_entry(
        &self,
        request: ah_vfs_storage::RemoveRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, ()> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Remove, true, move || {
            lookup::remove(&dir, &request, context)
        }))
    }

    fn move_entry(
        &self,
        request: ah_vfs_storage::RenameRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Rename, true, move || {
            lookup::rename(&dir, &request, context)
        }))
    }
}
