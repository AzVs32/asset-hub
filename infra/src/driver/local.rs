use std::io::{self, Read};

use asset_core::domain::Entry;
use asset_core::domain::{DriverPath, EntryName, VirtualRelativePath};
use asset_core::error::CoreError;
use asset_core::port::{BoundDriver, Driver};
use cap_std::ambient_authority;
use cap_std::fs::Dir;

/// A read-only driver for a directory on the local filesystem.
///
/// `DriverPath` is a filesystem directory path, resolved when `bind` is called.
/// Relative paths are resolved against the process working directory at that
/// time. Bound operations stay within the opened directory; symbolic links are
/// omitted from listings.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalDriver;

impl LocalDriver {
    pub fn new() -> Self {
        Self
    }
}

impl Driver for LocalDriver {
    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, CoreError> {
        if root.as_str().is_empty() {
            return Err(CoreError::driver_invalid_path());
        }

        let directory =
            Dir::open_ambient_dir(root.as_str(), ambient_authority()).map_err(bind_error)?;
        Ok(Box::new(LocalBoundDriver { directory }))
    }
}

struct LocalBoundDriver {
    directory: Dir,
}

impl BoundDriver for LocalBoundDriver {
    fn list(&self, path: &VirtualRelativePath) -> Result<Vec<Entry>, CoreError> {
        if path.is_empty() {
            list_directory(&self.directory)
        } else {
            let child = self
                .directory
                .open_dir(path.as_str())
                .map_err(driver_error)?;
            list_directory(&child)
        }
    }

    fn read(
        &self,
        path: &VirtualRelativePath,
    ) -> Result<Box<dyn Read + Send + 'static>, CoreError> {
        if path.is_empty() {
            return Err(CoreError::driver_is_directory());
        }

        let metadata = self
            .directory
            .metadata(path.as_str())
            .map_err(driver_error)?;
        if metadata.is_dir() {
            return Err(CoreError::driver_is_directory());
        }
        if !metadata.is_file() {
            return Err(unsupported_entry());
        }

        let file = self.directory.open(path.as_str()).map_err(driver_error)?;
        let metadata = file.metadata().map_err(driver_error)?;
        if metadata.is_dir() {
            return Err(CoreError::driver_is_directory());
        }
        if !metadata.is_file() {
            return Err(unsupported_entry());
        }
        Ok(Box::new(file))
    }
}

fn list_directory(directory: &Dir) -> Result<Vec<Entry>, CoreError> {
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
            .map_err(|_| CoreError::driver_unrepresentable_name())?;
        let name = EntryName::try_from(name.as_str())
            .map_err(|_| CoreError::driver_unrepresentable_name())?;
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

fn driver_error(error: io::Error) -> CoreError {
    match error.kind() {
        io::ErrorKind::NotFound => CoreError::driver_not_found(),
        io::ErrorKind::NotADirectory => CoreError::driver_not_directory(),
        _ => CoreError::backend(error),
    }
}

fn bind_error(error: io::Error) -> CoreError {
    if error.kind() == io::ErrorKind::InvalidInput {
        CoreError::driver_invalid_path()
    } else {
        driver_error(error)
    }
}

fn unsupported_entry() -> CoreError {
    CoreError::backend(io::Error::new(
        io::ErrorKind::Unsupported,
        "entry is not a regular file",
    ))
}
