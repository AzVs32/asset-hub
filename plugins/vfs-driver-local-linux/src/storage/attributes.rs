//! Attribute operations on pinned file/dir handles; paths never follow symlinks.
use super::LinuxLocalStorage;
use ah_vfs::domain::{StorageCapabilities, StorageError};
use cap_std::fs::Dir;

pub(super) fn instance_capabilities(dir: &Dir) -> Result<StorageCapabilities, StorageError> {
    let mut capabilities = LinuxLocalStorage::supported_capabilities();
    let handle = match dir.open(".") {
        Ok(handle) => handle,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return Ok(capabilities);
        }
        Err(error) => return Err(super::fs::error(error)),
    };
    let mut bytes = [0_u8; 0];
    if let Err(error) = rustix::fs::flistxattr(&handle, &mut bytes[..]) {
        if error == rustix::io::Errno::NOTSUP {
            capabilities.xattrs = ah_vfs::domain::AttributeSupport::None;
        } else if error != rustix::io::Errno::RANGE
            && error != rustix::io::Errno::ACCESS
            && error != rustix::io::Errno::PERM
        {
            return Err(super::fs::error(error.into()));
        }
    }
    Ok(capabilities)
}

mod posix {
    use super::super::{blocking, fs};
    use super::*;
    use ah_vfs::{
        application::port::{StorageFuture, StoragePermissions},
        domain::{EntryPermissions, StoragePath},
    };
    use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
    use cap_std::fs::{File, OpenOptions, Permissions, PermissionsExt};

    pub(super) type Handle = File;
    pub(super) fn open(root: &Dir, path: &StoragePath) -> Result<Handle, StorageError> {
        if path.is_root() {
            return root.open(".").map_err(fs::error);
        }
        let (parent, name) = fs::parent(root, path)?;
        let metadata = parent.symlink_metadata(&name).map_err(fs::error)?;
        if metadata.is_dir() {
            return parent
                .open_dir_nofollow(name)
                .map_err(fs::error)?
                .open(".")
                .map_err(fs::error);
        }
        fs::regular_file(&metadata)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = parent.open_with(name, &options).map_err(fs::error)?;
        fs::regular_file(&file.metadata().map_err(fs::error)?)?;
        Ok(file)
    }

    /// Attribute access requires enough access to open the file/dir for reading.
    impl StoragePermissions for LinuxLocalStorage {
        fn get_permissions(&self, path: StoragePath) -> StorageFuture<'_, EntryPermissions> {
            let dir = self.dir.clone();
            Box::pin(blocking(move || {
                EntryPermissions::new(
                    open(&dir, &path)?
                        .metadata()
                        .map_err(fs::error)?
                        .permissions()
                        .mode()
                        & 0o7777,
                )
            }))
        }
        fn set_permissions(
            &self,
            path: StoragePath,
            permissions: EntryPermissions,
        ) -> StorageFuture<'_, ()> {
            let dir = self.dir.clone();
            Box::pin(blocking(move || {
                open(&dir, &path)?
                    .set_permissions(Permissions::from_mode(permissions.bits()))
                    .map_err(fs::error)
            }))
        }
    }

    mod xattrs {
        use super::*;
        use ah_vfs::{
            application::port::StorageXattrs,
            domain::{StorageErrorKind, XattrName, XattrWriteMode},
        };
        use std::num::NonZeroUsize;

        fn xattr_error(error: rustix::io::Errno) -> StorageError {
            if error == rustix::io::Errno::RANGE {
                StorageError::with_source(StorageErrorKind::TooLarge, std::io::Error::from(error))
            } else {
                fs::error(error.into())
            }
        }
        /// Linux attribute values and name lists are bounded to 64 KiB per request.
        impl StorageXattrs for LinuxLocalStorage {
            fn list_xattrs(
                &self,
                path: StoragePath,
                max_bytes: NonZeroUsize,
            ) -> StorageFuture<'_, Vec<XattrName>> {
                let dir = self.dir.clone();
                Box::pin(blocking(move || {
                    let handle = open(&dir, &path)?;
                    let mut bytes = vec![0; max_bytes.get().min(64 * 1024)];
                    let len =
                        rustix::fs::flistxattr(&handle, &mut bytes[..]).map_err(xattr_error)?;
                    bytes.truncate(len);
                    let mut names = bytes
                        .split(|byte| *byte == 0)
                        .filter(|name| !name.is_empty())
                        .map(|name| {
                            let name = std::str::from_utf8(name).map_err(|error| {
                                StorageError::with_source(
                                    StorageErrorKind::InvalidNameEncoding,
                                    error,
                                )
                            })?;
                            XattrName::parse(name)
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
                let dir = self.dir.clone();
                Box::pin(blocking(move || {
                    let handle = open(&dir, &path)?;
                    let mut bytes = vec![0; max_bytes.get().min(64 * 1024)];
                    let len = rustix::fs::fgetxattr(&handle, name.as_str(), &mut bytes[..])
                        .map_err(xattr_error)?;
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
                let dir = self.dir.clone();
                Box::pin(blocking(move || {
                    if value.len() > 64 * 1024 {
                        return Err(StorageError::new(StorageErrorKind::TooLarge));
                    }
                    let handle = open(&dir, &path)?;
                    let flags = match mode {
                        XattrWriteMode::Upsert => rustix::fs::XattrFlags::empty(),
                        XattrWriteMode::CreateNew => rustix::fs::XattrFlags::CREATE,
                        XattrWriteMode::Replace => rustix::fs::XattrFlags::REPLACE,
                    };
                    rustix::fs::fsetxattr(&handle, name.as_str(), &value, flags)
                        .map_err(xattr_error)
                }))
            }
            fn remove_xattr(&self, path: StoragePath, name: XattrName) -> StorageFuture<'_, ()> {
                let dir = self.dir.clone();
                Box::pin(blocking(move || {
                    let handle = open(&dir, &path)?;
                    rustix::fs::fremovexattr(&handle, name.as_str()).map_err(xattr_error)
                }))
            }
        }
    }
}
