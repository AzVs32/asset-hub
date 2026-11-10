use super::{Blob, EntryData, MemoryEntry, Shared, error, metadata_error};
use ah_vfs::application::port::{StorageFileWriter, StorageFuture};
use ah_vfs_storage::{
    BackendFailure, DirectoryIdentity, Entry, Failure, Operation, Rejection, StorageError,
    StoragePath,
};
use std::{
    sync::{Arc, Weak},
    time::SystemTime,
};

struct WriteSession {
    shared: Arc<Shared>,
    path: StoragePath,
    parent_path: StoragePath,
    parent: Arc<DirectoryIdentity>,
    destination: Option<Weak<Blob>>,
    blob: Blob,
}

pub(super) struct MemoryFileWriter {
    session: Option<WriteSession>,
}

fn closed() -> StorageError {
    error(Operation::Write, Failure::Backend(BackendFailure::Closed))
}

impl MemoryFileWriter {
    pub(super) fn new(
        shared: Arc<Shared>,
        path: StoragePath,
        parent_path: StoragePath,
        parent: Arc<DirectoryIdentity>,
        destination: Option<Weak<Blob>>,
    ) -> Self {
        let blob = Blob {
            bytes: Vec::new(),
            quota: shared.quota.clone(),
        };
        Self {
            session: Some(WriteSession {
                shared,
                path,
                parent_path,
                parent,
                destination,
                blob,
            }),
        }
    }
}

impl StorageFileWriter for MemoryFileWriter {
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let mut session = self.session.take().ok_or_else(closed)?;
            session.blob.append(data).map_err(|cause| {
                StorageError::without_effects(
                    Operation::Write,
                    Failure::Backend(BackendFailure::CapacityExceeded),
                    Some(Box::new(cause)),
                )
            })?;
            self.session = Some(session);
            Ok(())
        })
    }

    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let op = Operation::Write;
            let session = self.session.take().ok_or_else(closed)?;
            let mut nodes = session.shared.lock(op)?;
            let parent_matches = matches!(nodes.get(session.parent_path.as_str()),
                Some(MemoryEntry { data: EntryData::Dir(actual), .. }) if Arc::ptr_eq(actual, &session.parent));
            let destination_matches = match (&session.destination, nodes.get(session.path.as_str()))
            {
                (None, None) => true,
                (
                    Some(expected),
                    Some(MemoryEntry {
                        data: EntryData::File(actual),
                        ..
                    }),
                ) => expected.ptr_eq(&Arc::downgrade(actual)),
                _ => false,
            };
            if !parent_matches || !destination_matches {
                return Err(error(op, Failure::Rejected(Rejection::Conflict)));
            }
            let entry = MemoryEntry {
                data: EntryData::File(Arc::new(session.blob)),
                modified: SystemTime::now(),
            };
            let metadata = entry
                .metadata(session.path.clone())
                .map_err(|cause| metadata_error(op, cause))?;
            nodes.insert(session.path.as_str().into(), entry);
            Ok(metadata)
        })
    }

    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move { self.session.take().map(|_| ()).ok_or_else(closed) })
    }
}
