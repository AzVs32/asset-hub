//! Directory-handle based local storage, with staged complete-file writes.

use ah_vfs::application::dto::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageEntry,
    StorageMoveEntryRequest, StorageReadFileRequest, StorageRemoveEntryRequest,
    StorageStatEntryRequest, StorageWriteFileRequest,
};
use ah_vfs::application::port::{Storage, StorageFileReader, StorageFileWriter, StorageFuture};
use ah_vfs::domain::StorageErrorKind;
use ah_vfs::domain::{
    AttributeSupport, DirModel, FileReadSupport, FileWriteSupport, RenameSupport,
    StorageCapabilities, StorageError,
};
use cap_std::fs::Dir;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod file_reader;
mod file_writer;
mod fs;
mod native;

const CHUNK_SIZE: usize = 64 * 1024;
// This namespace is inaccessible through Storage and omitted from listings.
const TEMP_PREFIX: &str = ".asset-hub-write-";

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, StorageError> + Send + 'static,
) -> Result<T, StorageError> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            let kind = if error.is_cancelled() {
                StorageErrorKind::Cancelled
            } else {
                StorageErrorKind::Backend
            };
            StorageError::with_source(kind, error)
        })?
}

/// An opened Windows directory capability. Requests reject reparse traversal.
///
/// Directory handles exclude delete sharing to prevent namespace replacement.
/// Streams retain their own file handles independently of this object.
pub struct WindowsLocalStorage {
    root: PathBuf,
    dir: Arc<Dir>,
    capabilities: StorageCapabilities,
}

impl WindowsLocalStorage {
    pub(crate) fn supported_capabilities() -> StorageCapabilities {
        StorageCapabilities {
            stat_entry: true,
            list_entries: true,
            read_file: FileReadSupport::Range,
            write_file: FileWriteSupport {
                create_new: true,
                replace: true,
                atomic_publish: true,
            },
            dir_model: DirModel::Hierarchical,
            persistent_empty_dirs: true,
            create_dir: true,
            remove_entry: true,
            rename: RenameSupport {
                no_replace: true,
                replace: true,
            },
            permissions: AttributeSupport::None,
            xattrs: AttributeSupport::None,
        }
    }
    pub(crate) async fn open(root: PathBuf) -> Result<Self, StorageError> {
        let path = root.clone();
        let (dir, capabilities) = blocking(move || {
            let dir =
                Dir::open_ambient_dir(path, cap_std::ambient_authority()).map_err(fs::error)?;
            let capabilities = Self::supported_capabilities();
            Ok((dir, capabilities))
        })
        .await?;
        Ok(Self {
            root,
            dir: Arc::new(dir),
            capabilities,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Storage for WindowsLocalStorage {
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }

    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(CHUNK_SIZE).unwrap_or(NonZeroUsize::MIN)
    }

    fn stat_entry(&self, request: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        let dir = self.dir.clone();
        Box::pin(blocking(move || fs::stat_entry(&dir, request.path)))
    }

    fn list_entries(&self, query: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        let dir = self.dir.clone();
        Box::pin(blocking(move || fs::list_entries(&dir, query)))
    }

    fn read_file(
        &self,
        request: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        let dir = self.dir.clone();
        Box::pin(blocking(move || {
            Ok(Box::new(file_reader::LocalFileReader::open(&dir, request)?)
                as Box<dyn StorageFileReader>)
        }))
    }

    fn write_file(
        &self,
        request: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        let dir = self.dir.clone();
        Box::pin(blocking(move || {
            Ok(Box::new(file_writer::LocalFileWriter::open(&dir, request)?)
                as Box<dyn StorageFileWriter>)
        }))
    }

    fn create_dir(&self, request: StorageCreateDirRequest) -> StorageFuture<'_, StorageEntry> {
        let dir = self.dir.clone();
        Box::pin(blocking(move || fs::create_dir(&dir, request)))
    }

    fn remove_entry(&self, request: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        let dir = self.dir.clone();
        Box::pin(blocking(move || fs::remove_entry(&dir, request.path)))
    }

    fn move_entry(&self, request: StorageMoveEntryRequest) -> StorageFuture<'_, StorageEntry> {
        let support = self.capabilities.rename;
        if !(if request.replace {
            support.replace
        } else {
            support.no_replace
        }) {
            return Box::pin(async {
                Err(StorageError::new(StorageErrorKind::UnsupportedOperation))
            });
        }
        let dir = self.dir.clone();
        Box::pin(blocking(move || fs::move_entry(&dir, request)))
    }
}
