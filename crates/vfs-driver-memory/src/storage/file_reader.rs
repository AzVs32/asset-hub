use super::{Blob, CHUNK_SIZE};
use ah_vfs::application::port::{StorageFileReader, StorageFuture};
use ah_vfs_storage::{BackendFailure, Failure, Operation, ReadRange, StorageError};
use std::sync::Arc;

pub(super) struct MemoryFileReader {
    blob: Option<Arc<Blob>>,
    position: usize,
    end: usize,
}

impl MemoryFileReader {
    pub(super) fn new(blob: Arc<Blob>, range: ReadRange) -> Self {
        let position = usize::try_from(range.offset())
            .unwrap_or(usize::MAX)
            .min(blob.bytes.len());
        let length = range
            .length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(usize::MAX)
            .min(blob.bytes.len() - position);
        Self {
            blob: (length != 0).then_some(blob),
            position,
            end: position + length,
        }
    }

    fn next_bytes(&mut self) -> Result<Option<Vec<u8>>, std::collections::TryReserveError> {
        let Some(blob) = self.blob.as_ref() else {
            return Ok(None);
        };
        let next = self.position + (self.end - self.position).min(CHUNK_SIZE);
        let mut chunk = Vec::new();
        chunk.try_reserve_exact(next - self.position)?;
        chunk.extend_from_slice(&blob.bytes[self.position..next]);
        self.position = next;
        if next == self.end {
            self.blob = None;
        }
        Ok(Some(chunk))
    }
}

impl StorageFileReader for MemoryFileReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            self.next_bytes().map_err(|source| {
                StorageError::without_effects(
                    Operation::Read,
                    Failure::Backend(BackendFailure::CapacityExceeded),
                    Some(Box::new(source)),
                )
            })
        })
    }
}
