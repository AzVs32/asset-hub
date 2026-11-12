//! Directory-handle based local storage, with staged complete-file writes.

use ah_vfs::application::port::{DirectoryBindings, OperationLookup, Storage, StorageFuture};
use ah_vfs_storage::{
    BackendFailure, BoundListRequest, Capabilities, DirectoryCursor, DirectoryPage,
    DirectorySelection, Failure, Operation, Rejection, StorageError,
};

use cap_std::fs::Dir;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod file_reader;
mod file_writer;
mod fs;
mod lookup;
mod native;

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

/// An opened Windows directory capability. Requests reject reparse traversal.
///
/// Directory handles exclude delete sharing to prevent namespace replacement.
/// Streams retain their own file handles independently of this object.
pub struct WindowsLocalStorage {
    root: PathBuf,
    dir: Arc<Dir>,
    directory_bindings: Arc<DirectoryBindings<Dir>>,
}

impl WindowsLocalStorage {
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

impl Storage for WindowsLocalStorage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(CHUNK_SIZE).unwrap_or(NonZeroUsize::MIN)
    }

    fn storage_capabilities(&self) -> Option<Capabilities> {
        Some(crate::driver::WindowsLocalDriver::supported_capabilities())
    }

    fn begin_lookup(&self, operation: Operation) -> StorageFuture<'_, Box<dyn OperationLookup>> {
        Box::pin(async move {
            if !crate::driver::WindowsLocalDriver::supported_capabilities().supports(operation) {
                return Err(StorageError::without_effects(
                    operation,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            Ok(Box::new(lookup::LocalLookup::new(
                self.dir.clone(),
                self.directory_bindings.clone(),
                operation,
            )) as Box<dyn OperationLookup>)
        })
    }

    fn resume_directory<'a>(
        &'a self,
        cursor: &'a DirectoryCursor,
    ) -> StorageFuture<'a, DirectorySelection> {
        let selection = cursor.selection().clone();
        let root = self.dir.clone();
        let bindings = self.directory_bindings.clone();
        Box::pin(blocking(Operation::List, false, move || {
            let dir = lookup::reopen_listing_directory(&root, &selection, true)?;
            bindings.verify(&selection, &dir, true, lookup::same_directory)?;
            Ok(selection)
        }))
    }

    fn list_page<'a>(&'a self, bound: &'a BoundListRequest) -> StorageFuture<'a, DirectoryPage> {
        let request = bound.request().clone();
        let resuming = bound.after().is_some();
        let selection = bound.selection().clone();
        let root = self.dir.clone();
        let bindings = self.directory_bindings.clone();
        Box::pin(blocking(Operation::List, false, move || {
            let dir = lookup::reopen_listing_directory(&root, &selection, resuming)?;
            bindings.verify(&selection, &dir, resuming, lookup::same_directory)?;
            let bound = request
                .bind_selection(selection)
                .map_err(|_| lookup::list_stale())?;
            let page = fs::list_entries(&dir, &bound)?;
            let current = lookup::reopen_listing_directory(&root, bound.selection(), resuming)?;
            if !lookup::same_directory(&dir, &current)? {
                return Err(lookup::list_replaced(resuming));
            }
            Ok(page)
        }))
    }
}
