use std::io;
use std::path::Path;

use asset_vfs::driver::{BoundDriver, Driver, DriverError, DriverKind, DriverPath, ReadDriver};
use asset_vfs::entry::Entry;
use asset_vfs::error::VfsError;
use asset_vfs::namespace::{EntryName, VirtualRelativePath};
use cap_std::ambient_authority;
use cap_std::fs::Dir;

/// A directory-listing driver for the local filesystem.
///
/// `DriverPath` must be an absolute filesystem directory path.
/// Bound operations stay within the opened directory; symbolic links are
/// omitted from listings.
/// Mounts using this driver allow descendant mounts.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalDriver;

impl LocalDriver {
    pub fn new() -> Self {
        Self
    }
}

impl Driver for LocalDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::try_from("local").expect("local is a valid driver kind")
    }

    fn allows_submounts(&self) -> bool {
        true
    }

    fn validate_path(&self, root: &DriverPath) -> Result<(), VfsError> {
        if !Path::new(root.as_str()).is_absolute() {
            return Err(DriverError::InvalidPath.into());
        }
        Ok(())
    }

    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, VfsError> {
        self.validate_path(root)?;
        let directory =
            Dir::open_ambient_dir(root.as_str(), ambient_authority()).map_err(bind_error)?;
        Ok(Box::new(LocalBoundDriver { directory }))
    }
}

struct LocalBoundDriver {
    directory: Dir,
}

impl BoundDriver for LocalBoundDriver {
    fn reader(&self) -> &dyn ReadDriver {
        self
    }
}

impl ReadDriver for LocalBoundDriver {
    fn list(&self, path: &VirtualRelativePath) -> Result<Vec<Entry>, VfsError> {
        if path.is_empty() {
            Ok(list_directory(&self.directory)?)
        } else {
            let child = self
                .directory
                .open_dir(path.as_str())
                .map_err(driver_error)?;
            Ok(list_directory(&child)?)
        }
    }
}

fn list_directory(directory: &Dir) -> Result<Vec<Entry>, DriverError> {
    let mut entries = Vec::new();
    for item in directory.entries().map_err(driver_error)? {
        let item = item.map_err(driver_error)?;
        let file_type = item.file_type().map_err(driver_error)?;
        if file_type.is_symlink() || (!file_type.is_file() && !file_type.is_dir()) {
            continue;
        }

        let name = item
            .file_name()
            .into_string()
            .map_err(|_| DriverError::UnrepresentableName)?;
        let name =
            EntryName::try_from(name.as_str()).map_err(|_| DriverError::UnrepresentableName)?;
        entries.push(if file_type.is_dir() {
            Entry::directory(name)
        } else {
            let metadata = item.metadata().map_err(driver_error)?;
            Entry::file(name, Some(metadata.len()))
        });
    }
    entries.sort_unstable_by(|left, right| left.name().cmp(right.name()));
    Ok(entries)
}

fn driver_error(error: io::Error) -> DriverError {
    match error.kind() {
        io::ErrorKind::NotFound => DriverError::NotFound,
        io::ErrorKind::NotADirectory => DriverError::NotDirectory,
        _ => DriverError::backend(error),
    }
}

fn bind_error(error: io::Error) -> DriverError {
    if error.kind() == io::ErrorKind::InvalidInput {
        DriverError::InvalidPath
    } else {
        driver_error(error)
    }
}
