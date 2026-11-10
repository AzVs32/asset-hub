//! Windows names, reparse-point policy and directory-relative filesystem I/O.
use super::{TEMP_PREFIX, native};
use ah_vfs::domain::storage::StorageMoveEntryRequest;
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use ah_vfs_storage::{Entry, EntryName, EntryType, StoragePath};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, Metadata, MetadataExt, OpenOptions, OpenOptionsExt};
use std::io;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
};

pub(super) fn error(operation: Operation, error: io::Error) -> StorageError {
    StorageError::without_effects(
        operation,
        crate::error::io_failure(&error),
        Some(Box::new(error)),
    )
}
pub(super) fn private_name(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with(TEMP_PREFIX)
}
pub(super) fn validate_name(operation: Operation, name: &str) -> Result<(), StorageError> {
    if private_name(name) {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::AccessDenied),
            None,
        ));
    }
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name.chars().any(|ch| {
            ch <= '\u{1f}' || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
    {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::UnsupportedName),
            None,
        ));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(*prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    if reserved {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::UnsupportedName),
            None,
        ));
    }
    Ok(())
}
fn validate(operation: Operation, path: &StoragePath) -> Result<(), StorageError> {
    for name in path.entry_names() {
        validate_name(operation, name.as_str())?;
    }
    Ok(())
}
pub(super) fn reparse(metadata: &Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
pub(super) fn checked_metadata(
    operation: Operation,
    parent: &Dir,
    name: &str,
) -> Result<Metadata, StorageError> {
    validate_name(operation, name)?;
    native::open_public(operation, parent, name, FILE_READ_ATTRIBUTES)?
        .metadata()
        .map_err(|cause| error(operation, cause))
}
fn dir(operation: Operation, root: &Dir, path: &StoragePath) -> Result<Dir, StorageError> {
    validate(operation, path)?;
    let mut current = root.try_clone().map_err(|cause| error(operation, cause))?;
    for name in path.entry_names() {
        let name = name.as_str();
        // Keep directory handles unshareable for deletion. Traversal opens the
        // reparse point itself and rejects it, including junctions and mount points.
        let mut options = OpenOptions::new();
        options
            .read(true)
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .follow(FollowSymlinks::No);
        let file = current
            .open_with(name, &options)
            .map_err(|cause| error(operation, cause))?;
        native::public_handle(operation, &file)?;
        let metadata = file.metadata().map_err(|cause| error(operation, cause))?;
        if reparse(&metadata) {
            return Err(StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::Unsupported),
                None,
            ));
        }
        if !metadata.is_dir() {
            return Err(StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::NotDirectory),
                None,
            ));
        }
        current = Dir::from_std_file(file.into_std());
    }
    Ok(current)
}

pub(super) fn parent(
    operation: Operation,
    root: &Dir,
    path: &StoragePath,
) -> Result<(Dir, String), StorageError> {
    validate(operation, path)?;
    let parent = path.parent().ok_or_else(|| {
        StorageError::without_effects(operation, Failure::Rejected(Rejection::RootProtected), None)
    })?;
    let name = path
        .name()
        .ok_or_else(|| {
            StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::RootProtected),
                None,
            )
        })?
        .as_str()
        .to_owned();
    Ok((dir(operation, root, &parent)?, name))
}
pub(super) fn entry(
    operation: Operation,
    path: StoragePath,
    metadata: Metadata,
) -> Result<Entry, StorageError> {
    let kind = if metadata.file_type().is_symlink() {
        EntryType::Symlink
    } else if reparse(&metadata) {
        EntryType::Other
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
pub(super) fn regular_file(operation: Operation, metadata: &Metadata) -> Result<(), StorageError> {
    if reparse(metadata) {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::Unsupported),
            None,
        ));
    }
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
fn list_path_error(operation: Operation, cause: ah_vfs_storage::PathError) -> StorageError {
    StorageError::without_effects(
        operation,
        Failure::Rejected(Rejection::UnsupportedName),
        Some(Box::new(cause)),
    )
}

pub(super) fn list_entries(
    dir: &Dir,
    bound: &ah_vfs_storage::BoundListRequest<'_>,
) -> Result<ah_vfs_storage::DirectoryPage, StorageError> {
    let operation = Operation::List;
    let address = bound.selection().address();
    let mut names = std::collections::BTreeSet::new();
    for child in dir.entries().map_err(|cause| error(operation, cause))? {
        let name = child
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
        if private_name(&name) {
            continue;
        }
        validate_name(operation, &name).map_err(|_| {
            StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::UnrepresentableName),
                None,
            )
        })?;
        let name = EntryName::parse(&name).map_err(|cause| {
            StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::UnrepresentableName),
                Some(Box::new(cause)),
            )
        })?;
        address
            .join(&name)
            .map_err(|cause| list_path_error(operation, cause))?;
        if bound.after().is_none_or(|after| &name > after) {
            names.insert(name);
            if names.len() > bound.size().get() + 1 {
                names.pop_last();
            }
        }
    }
    let more = names.len() > bound.size().get();
    let entries = names
        .into_iter()
        .take(bound.size().get())
        .map(|name| {
            entry(
                operation,
                address
                    .join(&name)
                    .map_err(|cause| list_path_error(operation, cause))?,
                checked_metadata(operation, dir, name.as_str())?,
            )
        })
        .collect::<Result<_, StorageError>>()?;
    ah_vfs_storage::DirectoryPage::new(bound, entries, more).map_err(|cause| {
        StorageError::without_effects(
            operation,
            Failure::Contract(ContractViolation::InvalidPage),
            Some(Box::new(cause)),
        )
    })
}
pub(super) fn remove_entry(root: &Dir, path: StoragePath) -> Result<(), StorageError> {
    let operation = Operation::Remove;
    let (parent, name) = parent(operation, root, &path)?;
    let file = native::mutation_handle(operation, &parent, &name)?;
    if reparse(&file.metadata().map_err(|cause| error(operation, cause))?) {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::Unsupported),
            None,
        ));
    }
    native::delete(operation, &file)
}
pub(super) fn move_entry(
    root: &Dir,
    request: StorageMoveEntryRequest,
) -> Result<Entry, StorageError> {
    let operation = Operation::Rename;
    let (source_parent, source) = parent(operation, root, &request.source)?;
    let (destination_parent, destination) = parent(operation, root, &request.destination)?;
    let file = native::mutation_handle(operation, &source_parent, &source)?;
    let metadata = file.metadata().map_err(|cause| error(operation, cause))?;
    if reparse(&metadata) {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::Unsupported),
            None,
        ));
    }
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
    native::rename(
        operation,
        &file,
        &destination_parent,
        &destination,
        request.replace,
    )?;
    entry(operation, request.destination, metadata)
}

pub(super) fn dispatched_error(operation: Operation, source: io::Error) -> StorageError {
    StorageError::after_dispatch(
        operation,
        crate::error::io_failure(&source),
        Some(Box::new(source)),
    )
}
