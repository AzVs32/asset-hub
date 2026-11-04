use super::{Blob, CHUNK_SIZE, error};
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
    pub(super) fn new(blob: Arc<Blob>, position: usize, end: usize) -> Self {
        Self {
            blob: Some(blob),
            position,
            end,
        }
    }
}

impl StorageFileReader for MemoryFileReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            if self.position == self.end {
                self.blob = None;
                return Ok(None);
            }
            let blob = self
                .blob
                .as_ref()
                .ok_or_else(|| error(StorageErrorKind::SessionClosed))?;
            let next = self.position + (self.end - self.position).min(CHUNK_SIZE);
            let mut chunk = Vec::new();
            chunk
                .try_reserve_exact(next - self.position)
                .map_err(|source| {
                    ah_vfs::domain::storage::StorageError::with_source(
                        StorageErrorKind::CapacityExceeded,
                        source,
                    )
                })?;
            chunk.extend_from_slice(&blob.bytes[self.position..next]);
            self.position = next;
            Ok(Some(chunk))
        })
    }
}
