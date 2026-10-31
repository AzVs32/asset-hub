//! Relative directory traversal and backend-neutral error/metadata conversion.

use super::TEMP_PREFIX;
use ah_vfs::application::dto::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageEntry, StorageEntryKind,
    StorageMoveEntryRequest,
};
use ah_vfs::domain::{StorageError, StorageErrorKind, StoragePath};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
use std::io;

pub(super) fn error(error: io::Error) -> StorageError {
    StorageError::with_source(crate::error::io_error_kind(&error), error)
}

fn validate(path: &StoragePath) -> Result<(), StorageError> {
    if path.segments().any(|name| name.starts_with(TEMP_PREFIX)) {
        return Err(StorageError::new(StorageErrorKind::PermissionDenied));
    }
    Ok(())
}

pub(super) fn dir(root: &Dir, path: &StoragePath, create: bool) -> Result<Dir, StorageError> {
    validate(path)?;
    let mut current = root.try_clone().map_err(error)?;
    for name in path.segments() {
        if create {
            match current.create_dir(name) {
                Ok(()) => (),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                Err(cause) => return Err(error(cause)),
            }
        }
        if current
            .symlink_metadata(name)
            .map_err(error)?
            .file_type()
            .is_symlink()
        {
            return Err(StorageError::new(StorageErrorKind::UnsupportedOperation));
        }
        current = current.open_dir_nofollow(name).map_err(error)?;
    }
    Ok(current)
}

pub(super) fn parent(root: &Dir, path: &StoragePath) -> Result<(Dir, String), StorageError> {
    validate(path)?;
    let parent = path
        .parent()
        .ok_or(StorageError::new(StorageErrorKind::RootProtected))?;
    let name = path
        .name()
        .ok_or(StorageError::new(StorageErrorKind::RootProtected))?
        .to_owned();
    Ok((dir(root, &parent, false)?, name))
}

pub(super) fn entry(path: StoragePath, metadata: Metadata) -> StorageEntry {
    let kind = if metadata.file_type().is_symlink() {
        StorageEntryKind::Symlink
    } else if metadata.is_file() {
        StorageEntryKind::File
    } else if metadata.is_dir() {
        StorageEntryKind::Dir
    } else {
        StorageEntryKind::Other
    };
    StorageEntry {
        path,
        kind,
        size: metadata.is_file().then(|| metadata.len()),
        modified: metadata.modified().ok().map(|time| time.into_std()),
    }
}

pub(super) fn stat_entry(root: &Dir, path: StoragePath) -> Result<StorageEntry, StorageError> {
    let metadata = if path.is_root() {
        root.dir_metadata().map_err(error)?
    } else {
        let (parent, name) = parent(root, &path)?;
        parent.symlink_metadata(name).map_err(error)?
    };
    Ok(entry(path, metadata))
}

pub(super) fn regular_file(metadata: &Metadata) -> Result<(), StorageError> {
    if metadata.is_dir() {
        return Err(StorageError::new(StorageErrorKind::IsDir));
    }
    if !metadata.is_file() {
        return Err(StorageError::new(StorageErrorKind::UnsupportedOperation));
    }
    Ok(())
}

pub(super) fn open_file(root: &Dir, path: &StoragePath) -> Result<File, StorageError> {
    if path.is_root() {
        return Err(StorageError::new(StorageErrorKind::IsDir));
    }
    let (parent, name) = parent(root, path)?;
    regular_file(&parent.symlink_metadata(&name).map_err(error)?)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent.open_with(name, &options).map_err(error)?;
    regular_file(&file.metadata().map_err(error)?)?;
    Ok(file)
}

pub(super) fn list_entries(
    root: &Dir,
    query: StorageDirQuery,
) -> Result<StorageDirPage, StorageError> {
    query.validate()?;
    let dir = dir(root, &query.path, false)?;
    let mut names = Vec::new();
    for entry in dir.entries().map_err(error)? {
        let name = entry
            .map_err(error)?
            .file_name()
            .into_string()
            .map_err(|_| StorageError::new(StorageErrorKind::InvalidNameEncoding))?;
        if name.starts_with(TEMP_PREFIX) {
            continue;
        }
        query.path.join(&name)?;
        if query.after.as_ref().is_none_or(|after| name > *after) {
            names.push(name);
        }
    }
    names.sort();
    let more = names.len() > query.limit.get();
    names.truncate(query.limit.get());
    let next_after = more.then(|| names.last().cloned()).flatten();
    let entries = names
        .into_iter()
        .map(|name| {
            let metadata = dir.symlink_metadata(&name).map_err(error)?;
            Ok(entry(query.path.join(&name)?, metadata))
        })
        .collect::<Result<_, StorageError>>()?;
    Ok(StorageDirPage {
        entries,
        next_after,
    })
}

pub(super) fn create_dir(
    root: &Dir,
    request: StorageCreateDirRequest,
) -> Result<StorageEntry, StorageError> {
    if request.path.is_root() && !request.recursive {
        return Err(StorageError::new(StorageErrorKind::AlreadyExists));
    }
    if request.recursive {
        let dir = dir(root, &request.path, true)?;
        Ok(entry(request.path, dir.dir_metadata().map_err(error)?))
    } else {
        let (parent, name) = parent(root, &request.path)?;
        parent.create_dir(&name).map_err(error)?;
        Ok(entry(
            request.path,
            parent.symlink_metadata(name).map_err(error)?,
        ))
    }
}

pub(super) fn remove_entry(root: &Dir, path: StoragePath) -> Result<(), StorageError> {
    let (parent, name) = parent(root, &path)?;
    let metadata = parent.symlink_metadata(&name).map_err(error)?;
    if metadata.is_dir() {
        parent.remove_dir(name).map_err(error)
    } else {
        parent.remove_file_or_symlink(name).map_err(error)
    }
}

pub(super) fn move_entry(
    root: &Dir,
    request: StorageMoveEntryRequest,
) -> Result<StorageEntry, StorageError> {
    let (source_parent, source) = parent(root, &request.source)?;
    let (destination_parent, destination) = parent(root, &request.destination)?;
    let metadata = source_parent.symlink_metadata(&source).map_err(error)?;
    if request.source == request.destination {
        if !request.replace {
            return Err(StorageError::new(StorageErrorKind::AlreadyExists));
        }
        return Ok(entry(request.destination, metadata));
    }
    if request.replace {
        source_parent
            .rename(source, &destination_parent, destination)
            .map_err(error)?;
    } else {
        rename_noreplace(&source_parent, &source, &destination_parent, &destination)?;
    }
    Ok(entry(request.destination, metadata))
}

fn rename_noreplace(
    source: &Dir,
    name: &str,
    destination: &Dir,
    target: &str,
) -> Result<(), StorageError> {
    rustix::fs::renameat_with(
        source,
        name,
        destination,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|cause| error(cause.into()))
}
