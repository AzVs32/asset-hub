//! Directory-handle based local storage, with staged complete-file writes.

use ah_vfs::application::port::{
    Storage, StorageFileReader, StorageFileWriter, StorageFuture, StoragePermissions, StorageXattrs,
};
use ah_vfs::domain::storage::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageMoveEntryRequest,
    StorageReadFileRequest, StorageRemoveEntryRequest, StorageStatEntryRequest,
    StorageWriteFileRequest,
};
use ah_vfs_storage::Entry;
use ah_vfs_storage::{BackendFailure, Failure, Operation, StorageError};

use cap_std::fs::Dir;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod attributes;
mod file_reader;
mod file_writer;
mod fs;

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

/// An opened directory capability. Requests never follow symbolic links.
///
/// Directory handles pin the accessed directories even if paths are renamed.
/// Reads and writes retain their own resources independently of this object.
pub struct LinuxLocalStorage {
    root: PathBuf,
    dir: Arc<Dir>,
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
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Storage for LinuxLocalStorage {
    fn permissions(&self) -> Option<&dyn StoragePermissions> {
        Some(self)
    }

    fn xattrs(&self) -> Option<&dyn StorageXattrs> {
        Some(self)
    }
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(CHUNK_SIZE).unwrap_or(NonZeroUsize::MIN)
    }

    fn stat_entry(&self, request: StorageStatEntryRequest) -> StorageFuture<'_, Entry> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Stat, false, move || {
            fs::stat_entry(&dir, request.path)
        }))
    }

    fn list_entries(&self, query: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::List, false, move || {
            fs::list_entries(&dir, query)
        }))
    }

    fn read_file(
        &self,
        request: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Read, false, move || {
            Ok(Box::new(file_reader::LocalFileReader::open(&dir, request)?)
                as Box<dyn StorageFileReader>)
        }))
    }

    fn write_file(
        &self,
        request: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Write, false, move || {
            Ok(Box::new(file_writer::LocalFileWriter::open(&dir, request)?)
                as Box<dyn StorageFileWriter>)
        }))
    }

    fn create_dir(&self, request: StorageCreateDirRequest) -> StorageFuture<'_, Entry> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::CreateDirectory, true, move || {
            fs::create_dir(&dir, request)
        }))
    }

    fn remove_entry(&self, request: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Remove, true, move || {
            fs::remove_entry(&dir, request.path)
        }))
    }

    fn move_entry(&self, request: StorageMoveEntryRequest) -> StorageFuture<'_, Entry> {
        let dir = self.dir.clone();
        Box::pin(blocking(Operation::Rename, true, move || {
            fs::move_entry(&dir, request)
        }))
    }
}
