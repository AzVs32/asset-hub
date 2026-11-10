//! Staged Windows writes, published by handle and disposed by handle on abort.
use super::{TEMP_PREFIX, blocking, fs, lookup, native};
use ah_vfs::application::port::{LookupContext, StorageFileWriter, StorageFuture};
use ah_vfs_storage::{BackendFailure, Failure, MutationOutcome, Operation, Rejection, StorageError};
use ah_vfs_storage::{Entry, StoragePath};
use ah_vfs_storage::{WriteMode, WriteRequest};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{io::Write, sync::Arc};
use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES};

#[derive(ah_id::Id)]
#[id(generate, display)]
struct WriteId(ah_id::UuidV7);
pub(super) struct LocalFileWriter {
    session: Option<FileWriteSession>,
}
struct FileWriteSession {
    parent: Dir,
    file: Option<File>,
    target: String,
    path: StoragePath,
    _ancestors: Vec<(StoragePath, Dir)>,
    cleanup_stage: bool,
}

impl LocalFileWriter {
    pub(super) fn open(
        root: Arc<Dir>,
        request: WriteRequest,
        context: LookupContext,
    ) -> Result<Self, StorageError> {
        let operation = Operation::Write;
        // FileRenameInfo can refuse an existing target, but cannot compare its identity.
        if request.mode() != WriteMode::CreateNew {
            return Err(StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::Unsupported),
                None,
            ));
        }
        let lookup::WriteDestination {
            parent,
            path,
            ancestors,
        } = lookup::write_destination(&root, &request, context)?;
        let target = path
            .name()
            .ok_or_else(|| {
                StorageError::without_effects(
                    operation,
                    Failure::Rejected(Rejection::RootProtected),
                    None,
                )
            })?
            .as_str()
            .to_owned();
        let temporary = format!("{TEMP_PREFIX}{}", WriteId::new());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .access_mode(FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE)
            .share_mode(0);
        let file = parent
            .open_with(&temporary, &options)
            .map_err(|cause| fs::error(operation, cause))?;
        Ok(Self {
            session: Some(FileWriteSession {
                parent,
                file: Some(file),
                target,
                path,
                _ancestors: ancestors,
                cleanup_stage: true,
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
        file.sync_all()
            .map_err(|cause| fs::error(operation, cause))?;
        let entry = fs::entry(
            operation,
            self.path.clone(),
            file.metadata()
                .map_err(|cause| fs::error(operation, cause))?,
        )?;
        // After dispatch the handle may refer to the published file. Deleting
        // through it is safe only if the rename is known to have failed.
        self.cleanup_stage = false;
        if let Err(error) = native::rename(operation, file, &self.parent, &self.target, false) {
            if error.failure() == Failure::Rejected(Rejection::AlreadyExists) {
                self.cleanup_stage = true;
                return Err(StorageError::without_effects(
                    operation,
                    Failure::Rejected(Rejection::Conflict),
                    Some(Box::new(error)),
                ));
            }
            self.cleanup_stage = error.outcome() == MutationOutcome::Unchanged;
            return Err(error);
        }
        Ok(entry)
    }
    fn abort(mut self) -> Result<(), StorageError> {
        let operation = Operation::Write;
        let file = self.file.as_ref().ok_or_else(|| {
            StorageError::without_effects(operation, Failure::Backend(BackendFailure::Closed), None)
        })?;
        native::delete(operation, file)?;
        self.cleanup_stage = false;
        self.file.take();
        Ok(())
    }
}
impl Drop for FileWriteSession {
    fn drop(&mut self) {
        let operation = Operation::Write;
        if self.cleanup_stage && let Some(file) = self.file.take() {
            let _ = native::delete(operation, &file);
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
