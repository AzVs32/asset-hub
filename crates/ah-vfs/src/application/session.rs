//! Owned streams that keep backend resources alive independently of routing services.

use super::{
    dto::EntryView,
    error::EntryError,
    port::{StorageFileReader, StorageFileWriter},
};
use crate::domain::VirtualPath;
use std::num::NonZeroUsize;

pub struct FileReadHandle {
    reader: Box<dyn StorageFileReader>,
    chunk_size: NonZeroUsize,
}

impl FileReadHandle {
    pub(super) fn new(reader: Box<dyn StorageFileReader>, chunk_size: NonZeroUsize) -> Self {
        Self { reader, chunk_size }
    }
    pub fn chunk_size(&self) -> NonZeroUsize {
        self.chunk_size
    }
    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, EntryError> {
        Ok(self.reader.next_chunk().await?)
    }
}

/// Complete-file write with the terminal commit/abort semantics of StorageFileWriter.
pub struct FileWriteHandle {
    writer: Box<dyn StorageFileWriter>,
    chunk_size: NonZeroUsize,
    path: VirtualPath,
}

impl FileWriteHandle {
    pub(super) fn new(
        writer: Box<dyn StorageFileWriter>,
        chunk_size: NonZeroUsize,
        path: VirtualPath,
    ) -> Self {
        Self {
            writer,
            chunk_size,
            path,
        }
    }
    pub fn chunk_size(&self) -> NonZeroUsize {
        self.chunk_size
    }
    pub async fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), EntryError> {
        Ok(self.writer.write_chunk(bytes).await?)
    }
    pub async fn commit(&mut self) -> Result<EntryView, EntryError> {
        let entry = self.writer.commit().await?;
        Ok(EntryView::from_storage(self.path.clone(), entry))
    }
    pub async fn abort(&mut self) -> Result<(), EntryError> {
        Ok(self.writer.abort().await?)
    }
}
