//! Staged Windows writes, published by handle and disposed by handle on abort.
use super::{TEMP_PREFIX, blocking, fs, native};
use ah_vfs::application::port::{StorageFileWriter, StorageFuture};
use ah_vfs::domain::storage::StorageWriteFileRequest;
use ah_vfs_storage::WriteMode;
use ah_vfs_storage::{BackendFailure, Failure, Operation, Rejection, StorageError};
use ah_vfs_storage::{Entry, StoragePath};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{io::Write, sync::Arc};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, READ_CONTROL, WRITE_DAC,
};

#[derive(ah_id::Id)]
#[id(generate, display)]
struct WriteId(ah_id::UuidV7);
pub(super) struct LocalFileWriter {
    session: Option<FileWriteSession>,
}
struct FileWriteSession {
    parent: Arc<Dir>,
    file: Option<File>,
    target: String,
    path: StoragePath,
    mode: WriteMode,
    published: bool,
}

fn check_target(parent: &Dir, target: &str, mode: WriteMode) -> Result<Option<File>, StorageError> {
    let operation = Operation::Write;
    match fs::checked_metadata(operation, parent, target) {
        Ok(_) if mode == WriteMode::CreateNew => Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::AlreadyExists),
            None,
        )),
        Ok(metadata) => {
            fs::regular_file(operation, &metadata)?;
            let file = native::open_public(operation, parent, target, READ_CONTROL)?;
            fs::regular_file(
                operation,
                &file
                    .metadata()
                    .map_err(|cause| fs::error(operation, cause))?,
            )?;
            Ok(Some(file))
        }
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}
impl LocalFileWriter {
    pub(super) fn open(root: &Dir, request: StorageWriteFileRequest) -> Result<Self, StorageError> {
        let operation = Operation::Write;
        let (parent, target) = fs::parent(operation, root, &request.path)?;
        check_target(&parent, &target, request.mode)?;
        let temporary = format!("{TEMP_PREFIX}{}", WriteId::new());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .access_mode(FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE | WRITE_DAC)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
        let file = parent
            .open_with(&temporary, &options)
            .map_err(|cause| fs::error(operation, cause))?;
        Ok(Self {
            session: Some(FileWriteSession {
                parent: Arc::new(parent),
                file: Some(file),
                target,
                path: request.path,
                mode: request.mode,
                published: false,
            }),
        })
    }
}
impl FileWriteSession {
    fn commit(mut self) -> Result<Entry, StorageError> {
        let operation = Operation::Write;
        let file = self.file.as_ref().ok_or_else(|| {
            StorageError::without_effects(operation, Failure::Backend(BackendFailure::Closed), None)
        })?;
        if let Some(destination) = check_target(&self.parent, &self.target, self.mode)? {
            native::copy_dacl(operation, &destination, file)?;
        }
        file.sync_all()
            .map_err(|cause| fs::error(operation, cause))?;
        let entry = fs::entry(
            operation,
            self.path.clone(),
            file.metadata()
                .map_err(|cause| fs::error(operation, cause))?,
        )?;
        native::rename(
            operation,
            file,
            &self.parent,
            &self.target,
            self.mode == WriteMode::Replace,
        )?;
        self.published = true;
        Ok(entry)
    }
    fn abort(mut self) -> Result<(), StorageError> {
        let operation = Operation::Write;
        let file = self.file.as_ref().ok_or_else(|| {
            StorageError::without_effects(operation, Failure::Backend(BackendFailure::Closed), None)
        })?;
        native::delete(operation, file)?;
        self.published = true;
        self.file.take();
        Ok(())
    }
}
impl Drop for FileWriteSession {
    fn drop(&mut self) {
        let operation = Operation::Write;
        if let Some(file) = self.file.take() {
            if !self.published {
                let _ = native::delete(operation, &file);
            }
        }
    }
}
impl StorageFileWriter for LocalFileWriter {
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()> {
        let operation = Operation::Write;
        Box::pin(async move {
            let mut session = self.session.take().ok_or_else(|| {
                StorageError::without_effects(
                    operation,
                    Failure::Backend(BackendFailure::Closed),
                    None,
                )
            })?;
            let bytes = data.to_vec();
            self.session = Some(
                blocking(operation, false, move || {
                    session
                        .file
                        .as_mut()
                        .ok_or_else(|| {
                            StorageError::without_effects(
                                operation,
                                Failure::Backend(BackendFailure::Closed),
                                None,
                            )
                        })?
                        .write_all(&bytes)
                        .map_err(|cause| fs::error(operation, cause))?;
                    Ok(session)
                })
                .await?,
            );
            Ok(())
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        let operation = Operation::Write;
        Box::pin(async move {
            let session = self.session.take().ok_or_else(|| {
                StorageError::without_effects(
                    operation,
                    Failure::Backend(BackendFailure::Closed),
                    None,
                )
            })?;
            tokio::task::spawn_blocking(move || session.commit())
                .await
                .map_err(|error| {
                    StorageError::after_dispatch(
                        operation,
                        Failure::Backend(BackendFailure::Other),
                        Some(Box::new(error)),
                    )
                })?
        })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        let operation = Operation::Write;
        Box::pin(async move {
            let session = self.session.take().ok_or_else(|| {
                StorageError::without_effects(
                    operation,
                    Failure::Backend(BackendFailure::Closed),
                    None,
                )
            })?;
            blocking(operation, false, move || session.abort()).await
        })
    }
}
