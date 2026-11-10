//! Bounded chunk reads retaining the opened file independently of its storage.

use super::{CHUNK_SIZE, blocking, fs};
use ah_vfs::application::port::{StorageFileReader, StorageFuture};
use ah_vfs_storage::{BackendFailure, Failure, Operation, ReadRange, StorageError};

use cap_std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub(super) struct LocalFileReader {
    session: Option<FileReadSession>,
    eof: bool,
}

struct FileReadSession {
    file: File,
    remaining: Option<u64>,
}

impl LocalFileReader {
    pub(super) fn open(mut file: File, range: ReadRange) -> Result<Self, StorageError> {
        let operation = Operation::Read;
        let size = file
            .metadata()
            .map_err(|cause| fs::error(operation, cause))?
            .len();
        let eof = range.length() == Some(0) || range.offset() >= size;
        if !eof {
            file.seek(SeekFrom::Start(range.offset()))
                .map_err(|cause| fs::error(operation, cause))?;
        }
        Ok(Self {
            session: (!eof).then_some(FileReadSession {
                file,
                remaining: range.length(),
            }),
            eof,
        })
    }
}

impl StorageFileReader for LocalFileReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        let operation = Operation::Read;
        Box::pin(async move {
            if self.eof {
                return Ok(None);
            }
            let mut session = self.session.take().ok_or(StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::Closed),
                None,
            ))?;
            let (session, bytes) = blocking(operation, false, move || {
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
                Ok((session, (length != 0).then_some(bytes)))
            })
            .await?;
            self.eof = bytes.is_none() || session.remaining == Some(0);
            if !self.eof {
                self.session = Some(session);
            }
            Ok(bytes)
        })
    }
}
