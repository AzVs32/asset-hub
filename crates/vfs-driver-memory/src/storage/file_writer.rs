use super::{
    Blob, BlobError, EntryData, MemoryEntry, Shared, error, metadata_error, require_dir,
    writable_target,
};
use ah_vfs::application::port::{StorageFileWriter, StorageFuture};
use ah_vfs::domain::storage::StorageWriteFileRequest;
use ah_vfs_storage::WriteMode;
use ah_vfs_storage::{BackendFailure, Failure, Operation, Rejection, StorageError};
use ah_vfs_storage::{Entry, StoragePath};
use std::{sync::Arc, time::SystemTime};

struct WriteSession {
    shared: Arc<Shared>,
    path: StoragePath,
    parent: Arc<ah_vfs_storage::DirectoryIdentity>,
    mode: WriteMode,
    blob: Blob,
}

pub(super) struct MemoryFileWriter {
    session: Option<WriteSession>,
}

impl MemoryFileWriter {
    pub(super) fn new(
        shared: Arc<Shared>,
        request: StorageWriteFileRequest,
        parent: Arc<ah_vfs_storage::DirectoryIdentity>,
    ) -> Self {
        let blob = Blob {
            bytes: Vec::new(),
            quota: shared.quota.clone(),
        };
        Self {
            session: Some(WriteSession {
                shared,
                path: request.path,
                parent,
                mode: request.mode,
                blob,
            }),
        }
    }
}

impl StorageFileWriter for MemoryFileWriter {
    fn write_chunk<'a>(&'a mut self, bytes: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            self.session
                .as_mut()
                .ok_or_else(|| error(Operation::Write, Failure::Backend(BackendFailure::Closed)))?
                .blob
                .append(bytes)
                .map_err(|cause| match cause {
                    BlobError::Quota => error(
                        Operation::Write,
                        Failure::Backend(BackendFailure::CapacityExceeded),
                    ),
                    BlobError::Allocation(source) => StorageError::without_effects(
                        Operation::Write,
                        Failure::Backend(BackendFailure::CapacityExceeded),
                        Some(Box::new(source)),
                    ),
                })
        })
    }

    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let session = self
                .session
                .take()
                .ok_or_else(|| error(Operation::Write, Failure::Backend(BackendFailure::Closed)))?;
            let mut entries = session.shared.lock(Operation::Write)?;
            let parent = session.path.parent().ok_or_else(|| {
                error(
                    Operation::Write,
                    Failure::Rejected(Rejection::RootProtected),
                )
            })?;
            let identity = require_dir(Operation::Write, &entries, parent.as_str())
                .map_err(|_| error(Operation::Write, Failure::Rejected(Rejection::Conflict)))?;
            if !Arc::ptr_eq(identity, &session.parent) {
                return Err(error(
                    Operation::Write,
                    Failure::Rejected(Rejection::Conflict),
                ));
            }
            writable_target(Operation::Write, &entries, &session.path, session.mode)?;
            let entry = MemoryEntry {
                data: EntryData::File(Arc::new(session.blob)),
                modified: SystemTime::now(),
            };
            let metadata = entry
                .metadata(session.path.clone())
                .map_err(|cause| metadata_error(Operation::Write, cause))?;
            entries.insert(session.path.as_str().into(), entry);
            Ok(metadata)
        })
    }

    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            self.session
                .take()
                .ok_or_else(|| error(Operation::Write, Failure::Backend(BackendFailure::Closed)))?;
            Ok(())
        })
    }
}
