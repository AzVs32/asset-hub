use super::{Blob, EntryData, MemoryEntry, Shared, error, require_dir, writable_target};
use ah_vfs::{
    application::port::{StorageFileWriter, StorageFuture},
    domain::storage::{
        Entry, StorageErrorKind, StoragePath, StorageWriteFileMode, StorageWriteFileRequest,
    },
};
use std::{sync::Arc, time::SystemTime};

struct WriteSession {
    shared: Arc<Shared>,
    path: StoragePath,
    parent: Arc<()>,
    mode: StorageWriteFileMode,
    blob: Blob,
}

pub(super) struct MemoryFileWriter {
    session: Option<WriteSession>,
}

impl MemoryFileWriter {
    pub(super) fn new(
        shared: Arc<Shared>,
        request: StorageWriteFileRequest,
        parent: Arc<()>,
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
                .ok_or_else(|| error(StorageErrorKind::SessionClosed))?
                .blob
                .append(bytes)
        })
    }

    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let session = self
                .session
                .take()
                .ok_or_else(|| error(StorageErrorKind::SessionClosed))?;
            let mut entries = session.shared.lock()?;
            let parent = session
                .path
                .parent()
                .ok_or_else(|| error(StorageErrorKind::RootProtected))?;
            let identity = require_dir(&entries, parent.as_str())
                .map_err(|_| error(StorageErrorKind::Conflict))?;
            if !Arc::ptr_eq(identity, &session.parent) {
                return Err(error(StorageErrorKind::Conflict));
            }
            writable_target(&entries, &session.path, session.mode)?;
            let entry = MemoryEntry {
                data: EntryData::File(Arc::new(session.blob)),
                modified: SystemTime::now(),
            };
            let metadata = entry.metadata(session.path.clone());
            entries.insert(session.path.as_str().into(), entry);
            Ok(metadata)
        })
    }

    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            self.session
                .take()
                .ok_or_else(|| error(StorageErrorKind::SessionClosed))?;
            Ok(())
        })
    }
}
