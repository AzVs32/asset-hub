//! Staged Windows writes, published by handle and disposed by handle on abort.
use super::{TEMP_PREFIX, blocking, fs, native};
use ah_vfs::{
    application::port::{StorageFileWriter, StorageFuture},
    domain::storage::{
        Entry, StorageError, StorageErrorKind, StoragePath, StorageWriteFileMode,
        StorageWriteFileRequest,
    },
};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{io::Write, sync::Arc};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, READ_CONTROL, WRITE_DAC, DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE,
};

#[derive(ah_id::Id)]
#[id(generate, display)]
struct WriteId(ah_id::UuidV7);
pub(super) struct LocalFileWriter { session: Option<FileWriteSession> }
struct FileWriteSession {
    parent: Arc<Dir>, file: Option<File>, target: String,
    path: StoragePath, mode: StorageWriteFileMode, published: bool,
}

fn check_target(parent: &Dir, target: &str, mode: StorageWriteFileMode) -> Result<Option<File>, StorageError> {
    match fs::checked_metadata(parent, target) {
        Ok(_) if mode == StorageWriteFileMode::CreateNew => Err(StorageError::new(StorageErrorKind::AlreadyExists)),
        Ok(metadata) => {
            fs::regular_file(&metadata)?;
            let file = native::open_public(parent, target, READ_CONTROL)?;
            fs::regular_file(&file.metadata().map_err(fs::error)?)?;
            Ok(Some(file))
        }
        Err(error) if error.kind() == StorageErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
impl LocalFileWriter {
    pub(super) fn open(root: &Dir, request: StorageWriteFileRequest) -> Result<Self, StorageError> {
        let (parent, target) = fs::parent(root, &request.path)?;
        check_target(&parent, &target, request.mode)?;
        let temporary = format!("{TEMP_PREFIX}{}", WriteId::new());
        let mut options = OpenOptions::new();
        options.write(true).create_new(true)
            .access_mode(FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE | WRITE_DAC)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
        let file = parent.open_with(&temporary, &options).map_err(fs::error)?;
        Ok(Self { session: Some(FileWriteSession {
            parent: Arc::new(parent), file: Some(file), target, path: request.path,
            mode: request.mode, published: false,
        }) })
    }
}
impl FileWriteSession {
    fn commit(mut self) -> Result<Entry, StorageError> {
        let file = self.file.as_ref().ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
        if let Some(destination) = check_target(&self.parent, &self.target, self.mode)? {
            native::copy_dacl(&destination, file)?;
        }
        file.sync_all().map_err(fs::error)?;
        let entry = fs::entry(self.path.clone(), file.metadata().map_err(fs::error)?);
        native::rename(file, &self.parent, &self.target, self.mode == StorageWriteFileMode::Replace)?;
        self.published = true;
        Ok(entry)
    }
    fn abort(mut self) -> Result<(), StorageError> {
        let file = self.file.as_ref().ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
        native::delete(file)?;
        self.published = true;
        self.file.take();
        Ok(())
    }
}
impl Drop for FileWriteSession {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            if !self.published { let _ = native::delete(&file); }
        }
    }
}
impl StorageFileWriter for LocalFileWriter {
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let mut session = self.session.take().ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
            let bytes = data.to_vec();
            self.session = Some(blocking(move || {
                session.file.as_mut().ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?
                    .write_all(&bytes).map_err(fs::error)?;
                Ok(session)
            }).await?);
            Ok(())
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let session = self.session.take().ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
            tokio::task::spawn_blocking(move || session.commit()).await
                .map_err(|error| StorageError::with_source(StorageErrorKind::OutcomeUnknown, error))?
        })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let session = self.session.take().ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
            blocking(move || session.abort()).await
        })
    }
}
