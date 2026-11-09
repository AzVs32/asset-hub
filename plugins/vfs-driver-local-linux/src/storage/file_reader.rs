//! Bounded chunk reads retaining the opened file independently of its storage.

use super::{CHUNK_SIZE, blocking, fs};
use ah_vfs::{
    application::port::{StorageFileReader, StorageFuture},
    domain::storage::StorageReadFileRequest,
};
use ah_vfs_storage::{BackendFailure, Failure, Operation, StorageError};

use cap_std::fs::{Dir, File};
use std::io::{Read, Seek, SeekFrom};

pub(super) struct LocalFileReader {
    session: Option<FileReadSession>,
}

struct FileReadSession {
    file: File,
    remaining: Option<u64>,
    eof: bool,
}

impl LocalFileReader {
    pub(super) fn open(root: &Dir, request: StorageReadFileRequest) -> Result<Self, StorageError> {
        let operation = Operation::Read;
        let mut file = fs::open_file(operation, root, &request.path)?;
        let mut eof = request.length == Some(0);
        if !eof {
            match file.seek(SeekFrom::Start(request.offset)) {
                Ok(_) => (),
                // Native seeks may reject u64 offsets that still denote empty ranges.
                Err(cause)
                    if cause.kind() == std::io::ErrorKind::InvalidInput
                        && request.offset
                            >= file
                                .metadata()
                                .map_err(|cause| fs::error(operation, cause))?
                                .len() =>
                {
                    eof = true;
                }
                Err(cause) => return Err(fs::error(operation, cause)),
            }
        }
        Ok(Self {
            session: Some(FileReadSession {
                file,
                remaining: request.length,
                eof,
            }),
        })
    }
}

impl StorageFileReader for LocalFileReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        let operation = Operation::Read;
        Box::pin(async move {
            let mut session = self.session.take().ok_or(StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::Closed),
                None,
            ))?;
            let (session, bytes) = blocking(operation, false, move || {
                if session.eof || session.remaining == Some(0) {
                    return Ok((session, None));
                }
                let size = session
                    .remaining
                    .map_or(CHUNK_SIZE, |left| left.min(CHUNK_SIZE as u64) as usize);
                let mut bytes = vec![0; size];
                let length = session
                    .file
                    .read(&mut bytes)
                    .map_err(|cause| fs::error(operation, cause))?;
                bytes.truncate(length);
                session.remaining = session.remaining.map(|left| left - length as u64);
                session.eof = length == 0;
                Ok((session, (length != 0).then_some(bytes)))
            })
            .await?;
            self.session = Some(session);
            Ok(bytes)
        })
    }
}
