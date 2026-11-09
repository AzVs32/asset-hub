//! Attribute operations on pinned file/dir handles; paths never follow symlinks.
use super::LinuxLocalStorage;
use ah_vfs_storage::{BackendFailure, Failure, Operation, Rejection, StorageError};

use cap_std::fs::Dir;

mod posix {
    use super::super::{blocking, fs};
    use super::*;
    use ah_vfs::application::port::{StorageFuture, StoragePermissions};

    use ah_vfs_storage::{Permissions, StoragePath};
    use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
    use cap_std::fs::{File, OpenOptions, Permissions as NativePermissions, PermissionsExt};

    pub(super) type Handle = File;
    pub(super) fn open(
        operation: Operation,
        root: &Dir,
        path: &StoragePath,
    ) -> Result<Handle, StorageError> {
        if path.is_root() {
            return root.open(".").map_err(|cause| fs::error(operation, cause));
        }
        let (parent, name) = fs::parent(operation, root, path)?;
        let metadata = parent
            .symlink_metadata(&name)
            .map_err(|cause| fs::error(operation, cause))?;
        if metadata.is_dir() {
            return parent
                .open_dir_nofollow(name)
                .map_err(|cause| fs::error(operation, cause))?
                .open(".")
                .map_err(|cause| fs::error(operation, cause));
        }
        fs::regular_file(operation, &metadata)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = parent
            .open_with(name, &options)
            .map_err(|cause| fs::error(operation, cause))?;
        fs::regular_file(
            operation,
            &file
                .metadata()
                .map_err(|cause| fs::error(operation, cause))?,
        )?;
        Ok(file)
    }

    /// Attribute access requires enough access to open the file/dir for reading.
    impl StoragePermissions for LinuxLocalStorage {
        fn get_permissions(&self, path: StoragePath) -> StorageFuture<'_, Permissions> {
            let operation = Operation::GetPermissions;
            let dir = self.dir.clone();
            Box::pin(blocking(operation, false, move || {
                Permissions::new(
                    open(operation, &dir, &path)?
                        .metadata()
                        .map_err(|cause| fs::error(operation, cause))?
                        .permissions()
                        .mode()
                        & 0o7777,
                )
                .map_err(|error| {
                    StorageError::without_effects(
                        operation,
                        Failure::Backend(BackendFailure::CorruptData),
                        Some(Box::new(error)),
                    )
                })
            }))
        }
        fn set_permissions(
            &self,
            path: StoragePath,
            permissions: Permissions,
        ) -> StorageFuture<'_, ()> {
            let operation = Operation::SetPermissions;
            let dir = self.dir.clone();
            Box::pin(blocking(operation, true, move || {
                open(operation, &dir, &path)?
                    .set_permissions(NativePermissions::from_mode(permissions.bits()))
                    .map_err(|cause| fs::dispatched_error(operation, cause))
            }))
        }
    }

    mod xattrs {
        use super::*;
        use ah_vfs::application::port::StorageXattrs;

        use ah_vfs_storage::{XattrName, XattrWriteMode};
        use std::num::NonZeroUsize;

        fn xattr_error(operation: Operation, error: rustix::io::Errno) -> StorageError {
            if error == rustix::io::Errno::RANGE {
                StorageError::without_effects(
                    operation,
                    Failure::Rejected(Rejection::LimitExceeded),
                    Some(Box::new(std::io::Error::from(error))),
                )
            } else {
                fs::error(operation, error.into())
            }
        }
        /// Linux attribute values and name lists are bounded to 64 KiB per request.
        impl StorageXattrs for LinuxLocalStorage {
            fn list_xattrs(
                &self,
                path: StoragePath,
                max_bytes: NonZeroUsize,
            ) -> StorageFuture<'_, Vec<XattrName>> {
                let operation = Operation::ListXattrs;
                let dir = self.dir.clone();
                Box::pin(blocking(operation, false, move || {
                    let handle = open(operation, &dir, &path)?;
                    let mut bytes = vec![0; max_bytes.get().min(64 * 1024)];
                    let len = rustix::fs::flistxattr(&handle, &mut bytes[..])
                        .map_err(|cause| xattr_error(operation, cause))?;
                    bytes.truncate(len);
                    let mut names = bytes
                        .split(|byte| *byte == 0)
                        .filter(|name| !name.is_empty())
                        .map(|name| {
                            let name = std::str::from_utf8(name).map_err(|error| {
                                StorageError::without_effects(
                                    operation,
                                    Failure::Backend(BackendFailure::UnrepresentableName),
                                    Some(Box::new(error)),
                                )
                            })?;
                            XattrName::parse(name).map_err(|error| {
                                StorageError::without_effects(
                                    operation,
                                    Failure::Backend(BackendFailure::CorruptData),
                                    Some(Box::new(error)),
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, StorageError>>()?;
                    names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                    Ok(names)
                }))
            }
            fn get_xattr(
                &self,
                path: StoragePath,
                name: XattrName,
                max_bytes: NonZeroUsize,
            ) -> StorageFuture<'_, Vec<u8>> {
                let operation = Operation::GetXattr;
                let dir = self.dir.clone();
                Box::pin(blocking(operation, false, move || {
                    let handle = open(operation, &dir, &path)?;
                    let mut bytes = vec![0; max_bytes.get().min(64 * 1024)];
                    let len = rustix::fs::fgetxattr(&handle, name.as_str(), &mut bytes[..])
                        .map_err(|cause| xattr_error(operation, cause))?;
                    bytes.truncate(len);
                    Ok(bytes)
                }))
            }
            fn set_xattr(
                &self,
                path: StoragePath,
                name: XattrName,
                value: Vec<u8>,
                mode: XattrWriteMode,
            ) -> StorageFuture<'_, ()> {
                let operation = Operation::SetXattr;
                let dir = self.dir.clone();
                Box::pin(blocking(operation, true, move || {
                    if value.len() > 64 * 1024 {
                        return Err(StorageError::without_effects(
                            operation,
                            Failure::Rejected(Rejection::LimitExceeded),
                            None,
                        ));
                    }
                    let handle = open(operation, &dir, &path)?;
                    let flags = match mode {
                        XattrWriteMode::Upsert => rustix::fs::XattrFlags::empty(),
                        XattrWriteMode::CreateNew => rustix::fs::XattrFlags::CREATE,
                        XattrWriteMode::Replace => rustix::fs::XattrFlags::REPLACE,
                    };
                    rustix::fs::fsetxattr(&handle, name.as_str(), &value, flags).map_err(|cause| {
                        StorageError::after_dispatch(
                            operation,
                            crate::error::io_failure(&std::io::Error::from(cause)),
                            Some(Box::new(std::io::Error::from(cause))),
                        )
                    })
                }))
            }
            fn remove_xattr(&self, path: StoragePath, name: XattrName) -> StorageFuture<'_, ()> {
                let operation = Operation::RemoveXattr;
                let dir = self.dir.clone();
                Box::pin(blocking(operation, true, move || {
                    let handle = open(operation, &dir, &path)?;
                    rustix::fs::fremovexattr(&handle, name.as_str()).map_err(|cause| {
                        StorageError::after_dispatch(
                            operation,
                            crate::error::io_failure(&std::io::Error::from(cause)),
                            Some(Box::new(std::io::Error::from(cause))),
                        )
                    })
                }))
            }
        }
    }
}
