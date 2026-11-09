//! Relative directory traversal and backend-neutral error/metadata conversion.

use super::TEMP_PREFIX;
use ah_vfs::domain::storage::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageMoveEntryRequest,
};
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use ah_vfs_storage::{Entry, EntryName, EntryType, StoragePath};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
use std::io;

pub(super) fn error(operation: Operation, error: io::Error) -> StorageError {
    StorageError::without_effects(
        operation,
        crate::error::io_failure(&error),
        Some(Box::new(error)),
    )
}

fn validate(operation: Operation, path: &StoragePath) -> Result<(), StorageError> {
    if path
        .entry_names()
        .any(|name| name.as_str().starts_with(TEMP_PREFIX))
    {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::AccessDenied),
            None,
        ));
    }
    Ok(())
}

pub(super) fn dir(
    operation: Operation,
    root: &Dir,
    path: &StoragePath,
    create: bool,
) -> Result<(Dir, bool), StorageError> {
    let mut changed = false;
    let result = (|| {
        validate(operation, path)?;
        let mut current = root.try_clone().map_err(|cause| error(operation, cause))?;
        for name in path.entry_names() {
            let name = name.as_str();
            if create {
                match current.create_dir(name) {
                    Ok(()) => changed = true,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                    Err(cause) => {
                        changed = true;
                        return Err(error(operation, cause));
                    }
                }
            }
            if current
                .symlink_metadata(name)
                .map_err(|cause| error(operation, cause))?
                .file_type()
                .is_symlink()
            {
                return Err(StorageError::without_effects(
                    operation,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            current = current
                .open_dir_nofollow(name)
                .map_err(|cause| error(operation, cause))?;
        }
        Ok(current)
    })();
    result
        .map(|dir| (dir, changed))
        .map_err(|error: StorageError| {
            if changed {
                StorageError::after_dispatch(operation, error.failure(), Some(Box::new(error)))
            } else {
                error
            }
        })
}

pub(super) fn parent(
    operation: Operation,
    root: &Dir,
    path: &StoragePath,
) -> Result<(Dir, String), StorageError> {
    validate(operation, path)?;
    let parent = path.parent().ok_or(StorageError::without_effects(
        operation,
        Failure::Rejected(Rejection::RootProtected),
        None,
    ))?;
    let name = path
        .name()
        .ok_or(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::RootProtected),
            None,
        ))?
        .as_str()
        .to_owned();
    Ok((dir(operation, root, &parent, false)?.0, name))
}

pub(super) fn entry(
    operation: Operation,
    path: StoragePath,
    metadata: Metadata,
) -> Result<Entry, StorageError> {
    let kind = if metadata.file_type().is_symlink() {
        EntryType::Symlink
    } else if metadata.is_file() {
        EntryType::file(Some(metadata.len()))
    } else if metadata.is_dir() {
        EntryType::Directory
    } else {
        EntryType::Other
    };
    Entry::new(
        path,
        kind,
        metadata.modified().ok().map(|time| time.into_std()),
    )
    .map_err(|cause| {
        StorageError::without_effects(
            operation,
            Failure::Contract(ContractViolation::InvalidResponse),
            Some(Box::new(cause)),
        )
    })
}

pub(super) fn stat_entry(root: &Dir, path: StoragePath) -> Result<Entry, StorageError> {
    let operation = Operation::Stat;
    let metadata = if path.is_root() {
        root.dir_metadata()
            .map_err(|cause| error(operation, cause))?
    } else {
        let (parent, name) = parent(operation, root, &path)?;
        parent
            .symlink_metadata(name)
            .map_err(|cause| error(operation, cause))?
    };
    entry(operation, path, metadata)
}

pub(super) fn regular_file(operation: Operation, metadata: &Metadata) -> Result<(), StorageError> {
    if metadata.is_dir() {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::IsDirectory),
            None,
        ));
    }
    if !metadata.is_file() {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::Unsupported),
            None,
        ));
    }
    Ok(())
}

pub(super) fn open_file(
    operation: Operation,
    root: &Dir,
    path: &StoragePath,
) -> Result<File, StorageError> {
    if path.is_root() {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::IsDirectory),
            None,
        ));
    }
    let (parent, name) = parent(operation, root, path)?;
    regular_file(
        operation,
        &parent
            .symlink_metadata(&name)
            .map_err(|cause| error(operation, cause))?,
    )?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent
        .open_with(name, &options)
        .map_err(|cause| error(operation, cause))?;
    regular_file(
        operation,
        &file.metadata().map_err(|cause| error(operation, cause))?,
    )?;
    Ok(file)
}

fn list_path_error(operation: Operation, cause: ah_vfs_storage::PathError) -> StorageError {
    StorageError::without_effects(
        operation,
        Failure::Rejected(Rejection::UnsupportedName),
        Some(Box::new(cause)),
    )
}

pub(super) fn list_entries(
    root: &Dir,
    query: StorageDirQuery,
) -> Result<StorageDirPage, StorageError> {
    let operation = Operation::List;
    query.validate()?;
    let dir = dir(operation, root, &query.path, false)?.0;
    let mut names = Vec::new();
    for entry in dir.entries().map_err(|cause| error(operation, cause))? {
        let name = entry
            .map_err(|cause| error(operation, cause))?
            .file_name()
            .into_string()
            .map_err(|_| {
                StorageError::without_effects(
                    operation,
                    Failure::Backend(BackendFailure::UnrepresentableName),
                    None,
                )
            })?;
        if name.starts_with(TEMP_PREFIX) {
            continue;
        }
        let name = EntryName::parse(&name).map_err(|cause| {
            StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::UnrepresentableName),
                Some(Box::new(cause)),
            )
        })?;
        query
            .path
            .join(&name)
            .map_err(|cause| list_path_error(operation, cause))?;
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
            let metadata = dir
                .symlink_metadata(name.as_str())
                .map_err(|cause| error(operation, cause))?;
            entry(
                operation,
                query
                    .path
                    .join(&name)
                    .map_err(|cause| list_path_error(operation, cause))?,
                metadata,
            )
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
) -> Result<Entry, StorageError> {
    let operation = Operation::CreateDirectory;
    if request.path.is_root() && !request.recursive {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::AlreadyExists),
            None,
        ));
    }
    if request.recursive {
        let (dir, changed) = dir(operation, root, &request.path, true)?;
        (|| {
            entry(
                operation,
                request.path,
                dir.dir_metadata()
                    .map_err(|cause| error(operation, cause))?,
            )
        })()
        .map_err(|error: StorageError| {
            if changed {
                StorageError::after_dispatch(operation, error.failure(), Some(Box::new(error)))
            } else {
                error
            }
        })
    } else {
        let (parent, name) = parent(operation, root, &request.path)?;
        parent
            .create_dir(&name)
            .map_err(|cause| dispatched_error(operation, cause))?;
        entry(
            operation,
            request.path,
            parent
                .symlink_metadata(name)
                .map_err(|cause| dispatched_error(operation, cause))?,
        )
        .map_err(|error| {
            StorageError::after_dispatch(operation, error.failure(), Some(Box::new(error)))
        })
    }
}

pub(super) fn remove_entry(root: &Dir, path: StoragePath) -> Result<(), StorageError> {
    let operation = Operation::Remove;
    let (parent, name) = parent(operation, root, &path)?;
    let metadata = parent
        .symlink_metadata(&name)
        .map_err(|cause| error(operation, cause))?;
    if metadata.is_dir() {
        parent
            .remove_dir(name)
            .map_err(|cause| dispatched_error(operation, cause))
    } else {
        parent
            .remove_file_or_symlink(name)
            .map_err(|cause| dispatched_error(operation, cause))
    }
}

pub(super) fn move_entry(
    root: &Dir,
    request: StorageMoveEntryRequest,
) -> Result<Entry, StorageError> {
    let operation = Operation::Rename;
    let (source_parent, source) = parent(operation, root, &request.source)?;
    let (destination_parent, destination) = parent(operation, root, &request.destination)?;
    let metadata = source_parent
        .symlink_metadata(&source)
        .map_err(|cause| error(operation, cause))?;
    if request.source == request.destination {
        if !request.replace {
            return Err(StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::AlreadyExists),
                None,
            ));
        }
        return entry(operation, request.destination, metadata);
    }
    if request.replace {
        source_parent
            .rename(source, &destination_parent, destination)
            .map_err(|cause| dispatched_error(operation, cause))?;
    } else {
        rename_noreplace(&source_parent, &source, &destination_parent, &destination)?;
    }
    entry(operation, request.destination, metadata).map_err(|error| {
        StorageError::after_dispatch(operation, error.failure(), Some(Box::new(error)))
    })
}

fn rename_noreplace(
    source: &Dir,
    name: &str,
    destination: &Dir,
    target: &str,
) -> Result<(), StorageError> {
    let operation = Operation::Rename;
    rustix::fs::renameat_with(
        source,
        name,
        destination,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|cause| dispatched_error(operation, cause.into()))
}

pub(super) fn dispatched_error(operation: Operation, source: io::Error) -> StorageError {
    StorageError::after_dispatch(
        operation,
        crate::error::io_failure(&source),
        Some(Box::new(source)),
    )
}
