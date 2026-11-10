//! Stage anonymously and publish by linking the opened inode into an unused slot.
use super::{blocking, fs, lookup};
use ah_vfs::application::port::{LookupContext, StorageFileWriter, StorageFuture};
use ah_vfs_storage::{
    BackendFailure, Entry, Failure, Operation, Rejection, StorageError, StoragePath, WriteMode,
    WriteRequest,
};
use cap_std::fs::{Dir, File};
use rustix::fs::{AtFlags, Mode, OFlags, linkat, openat};
use rustix::io::Errno;
use std::{io::Write, os::fd::AsRawFd, sync::Arc};

const OP: Operation = Operation::Write;
fn closed() -> StorageError {
    StorageError::without_effects(OP, Failure::Backend(BackendFailure::Closed), None)
}

pub(super) struct LocalFileWriter {
    session: Option<FileWriteSession>,
}

struct FileWriteSession {
    parent: Dir,
    file: File,
    path: StoragePath,
}

impl LocalFileWriter {
    pub(super) fn open(
        root: Arc<Dir>,
        request: WriteRequest,
        context: LookupContext,
    ) -> Result<Self, StorageError> {
        // Linux has no rename conditional on the destination inode.
        if request.mode() != WriteMode::CreateNew {
            return Err(StorageError::without_effects(
                OP,
                Failure::Rejected(Rejection::Unsupported),
                None,
            ));
        }
        let (parent, path) = lookup::write_destination(&root, &request, context)?;
        // The instance-root descriptor is already the authoritative binding.
        // linkat cannot also attest that a nested parent still occupies its name.
        if !path.parent().is_some_and(|parent| parent.is_root()) {
            return Err(StorageError::without_effects(
                OP,
                Failure::Rejected(Rejection::Unsupported),
                None,
            ));
        }
        let fd = openat(
            &parent,
            ".",
            OFlags::TMPFILE | OFlags::WRONLY | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|cause| match cause {
            Errno::OPNOTSUPP | Errno::ISDIR | Errno::INVAL => StorageError::without_effects(
                OP,
                Failure::Rejected(Rejection::Unsupported),
                Some(Box::new(cause)),
            ),
            _ => fs::error(OP, cause.into()),
        })?;
        let file = File::from_std(fd.into());
        Ok(Self {
            session: Some(FileWriteSession { parent, file, path }),
        })
    }
}

impl FileWriteSession {
    fn commit(self) -> Result<Entry, StorageError> {
        self.file.sync_all().map_err(|cause| fs::error(OP, cause))?;
        let entry = fs::entry(
            OP,
            self.path.clone(),
            self.file.metadata().map_err(|cause| fs::error(OP, cause))?,
        )?;
        let target = self.path.name().ok_or_else(|| {
            StorageError::without_effects(OP, Failure::Rejected(Rejection::RootProtected), None)
        })?;
        let source = format!("/proc/self/fd/{}", self.file.as_raw_fd());
        linkat(
            rustix::fs::CWD,
            source.as_str(),
            &self.parent,
            target.as_str(),
            AtFlags::SYMLINK_FOLLOW,
        )
        .map_err(|cause| match cause {
            Errno::EXIST => StorageError::without_effects(
                OP,
                Failure::Rejected(Rejection::Conflict),
                Some(Box::new(cause)),
            ),
            Errno::NOENT => StorageError::without_effects(
                OP,
                Failure::Rejected(Rejection::Unsupported),
                Some(Box::new(cause)),
            ),
            _ => fs::dispatched_error(OP, cause.into()),
        })?;
        Ok(entry)
    }
}

impl StorageFileWriter for LocalFileWriter {
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let mut session = self.session.take().ok_or_else(closed)?;
            let bytes = data.to_vec();
            self.session = Some(
                blocking(OP, false, move || {
                    session
                        .file
                        .write_all(&bytes)
                        .map_err(|cause| fs::error(OP, cause))?;
                    Ok(session)
                })
                .await?,
            );
            Ok(())
        })
    }

    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let session = self.session.take().ok_or_else(closed)?;
            blocking(OP, true, move || session.commit()).await
        })
    }

    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move { self.session.take().map(|_| ()).ok_or_else(closed) })
    }
}
