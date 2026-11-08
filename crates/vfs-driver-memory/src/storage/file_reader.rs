use super::{Blob, CHUNK_SIZE};
use ah_vfs::{
    application::port::{StorageFileReader, StorageFuture},
    domain::storage::StorageErrorKind,
};
use std::sync::Arc;

pub(super) struct MemoryFileReader {
    blob: Option<Arc<Blob>>,
    position: usize,
    end: usize,
}

impl MemoryFileReader {
    pub(super) fn new(blob: Arc<Blob>, offset: u64, length: Option<u64>) -> Self {
        let position = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(blob.bytes.len());
        let length = length
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(usize::MAX)
            .min(blob.bytes.len() - position);
        Self {
            blob: Some(blob),
            position,
            end: position + length,
        }
    }

    pub(super) fn next_bytes(
        &mut self,
    ) -> Result<Option<Vec<u8>>, std::collections::TryReserveError> {
        if self.position == self.end {
            self.blob = None;
            return Ok(None);
        }
        let Some(blob) = self.blob.as_ref() else {
            return Ok(None);
        };
        let next = self.position + (self.end - self.position).min(CHUNK_SIZE);
        let mut chunk = Vec::new();
        chunk.try_reserve_exact(next - self.position)?;
        chunk.extend_from_slice(&blob.bytes[self.position..next]);
        self.position = next;
        Ok(Some(chunk))
    }
}

impl StorageFileReader for MemoryFileReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            self.next_bytes().map_err(|source| {
                ah_vfs::domain::storage::StorageError::with_source(
                    StorageErrorKind::CapacityExceeded,
                    source,
                )
            })
        })
    }
}
