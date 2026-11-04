//! Staged complete-file writes with explicit publication and best-effort cleanup.

use super::{TEMP_PREFIX, blocking, fs};
use ah_vfs::{
    application::port::{StorageFileWriter, StorageFuture},
    domain::storage::{
        Entry, StorageError, StorageErrorKind, StoragePath, StorageWriteFileMode,
        StorageWriteFileRequest,
    },
};

use cap_std::fs::OpenOptionsExt;
use cap_std::fs::{Dir, File, OpenOptions, Permissions};
use std::{
    io::{self, Write},
    sync::Arc,
};

#[derive(ah_id::Id)]
#[id(generate, display)]
struct WriteId(ah_id::UuidV7);

pub(super) struct LocalFileWriter {
    session: Option<FileWriteSession>,
}

struct FileWriteSession {
    parent: Arc<Dir>,
    temporary: Option<String>,
    file: Option<File>,
    target: String,
    path: StoragePath,
    mode: StorageWriteFileMode,
}

fn check_target(
    parent: &Dir,
    target: &str,
    mode: StorageWriteFileMode,
) -> Result<Option<Permissions>, StorageError> {
    match parent.symlink_metadata(target) {
        Ok(_) if mode == StorageWriteFileMode::CreateNew => {
            Err(StorageError::new(StorageErrorKind::AlreadyExists))
        }
        Ok(metadata) => {
            fs::regular_file(&metadata)?;
            Ok(Some(metadata.permissions()))
        }
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(cause) => Err(fs::error(cause)),
    }
}

impl LocalFileWriter {
    pub(super) fn open(root: &Dir, request: StorageWriteFileRequest) -> Result<Self, StorageError> {
        let (parent, target) = fs::parent(root, &request.path)?;
        check_target(&parent, &target, request.mode)?;
        let temporary = format!("{TEMP_PREFIX}{}", WriteId::new());
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        options.mode(0o600);
        let file = parent.open_with(&temporary, &options).map_err(fs::error)?;
        Ok(Self {
            session: Some(FileWriteSession {
                parent: Arc::new(parent),
                temporary: Some(temporary),
                file: Some(file),
                target,
                path: request.path,
                mode: request.mode,
            }),
        })
    }
}

impl FileWriteSession {
    fn commit(mut self) -> Result<Entry, StorageError> {
        let file = self
            .file
            .take()
            .ok_or(StorageError::new(StorageErrorKind::SessionClosed))?;
        // Preserve destination permissions when replacing a regular file.
        if let Some(permissions) = check_target(&self.parent, &self.target, self.mode)? {
            file.set_permissions(permissions).map_err(fs::error)?;
        }
        file.sync_all().map_err(fs::error)?;
        let entry = fs::entry(self.path.clone(), file.metadata().map_err(fs::error)?);
        drop(file);
        let temporary = self
            .temporary
            .as_ref()
            .ok_or(StorageError::new(StorageErrorKind::SessionClosed))?;
        match self.mode {
            StorageWriteFileMode::CreateNew => {
                // A hard link publishes without replacing a concurrently created target.
                self.parent
                    .hard_link(temporary, &self.parent, &self.target)
                    .map_err(fs::error)?;
                // Publication succeeded; cleanup failure must not report a false failed commit.
            }
            StorageWriteFileMode::Replace => {
                self.parent
                    .rename(temporary, &self.parent, &self.target)
                    .map_err(fs::error)?;
                self.temporary = None;
            }
        }
        Ok(entry)
    }

    fn abort(mut self) -> Result<(), StorageError> {
        drop(self.file.take());
        if let Some(temporary) = &self.temporary {
            match self.parent.remove_file(temporary) {
                Ok(()) => (),
                Err(cause) if cause.kind() == io::ErrorKind::NotFound => (),
                Err(cause) => return Err(fs::error(cause)),
            }
        }
        self.temporary = None;
        Ok(())
    }
}

impl Drop for FileWriteSession {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Some(temporary) = &self.temporary {
            let _ = self.parent.remove_file(temporary);
        }
    }
}

impl StorageFileWriter for LocalFileWriter {
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let mut session = self
                .session
                .take()
                .ok_or(StorageError::new(StorageErrorKind::SessionClosed))?;
            let bytes = data.to_vec();
            let session = blocking(move || {
                session
                    .file
                    .as_mut()
                    .ok_or(StorageError::new(StorageErrorKind::SessionClosed))?
                    .write_all(&bytes)
                    .map_err(fs::error)?;
                Ok(session)
            })
            .await?;
            self.session = Some(session);
            Ok(())
        })
    }

    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let session = self
                .session
                .take()
                .ok_or(StorageError::new(StorageErrorKind::SessionClosed))?;
            // A task failure can happen after publication; do not report a known rollback.
            tokio::task::spawn_blocking(move || session.commit())
                .await
                .map_err(|error| {
                    StorageError::with_source(StorageErrorKind::OutcomeUnknown, error)
                })?
        })
    }

    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let session = self
                .session
                .take()
                .ok_or(StorageError::new(StorageErrorKind::SessionClosed))?;
            blocking(move || session.abort()).await
        })
    }
}
