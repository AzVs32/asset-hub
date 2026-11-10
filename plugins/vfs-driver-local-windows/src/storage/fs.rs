//! Windows names, reparse-point policy and directory-relative filesystem I/O.
use super::{TEMP_PREFIX, native};
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use ah_vfs_storage::{Entry, EntryName, EntryType, StoragePath};
use cap_std::fs::{Dir, Metadata, MetadataExt};
use std::io;
use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_READ_ATTRIBUTES};

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
            Failure::Contract(ContractViolation::InvalidEntry),
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
        Failure::Rejected(Rejection::PathTooLong),
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
pub(super) fn dispatched_error(operation: Operation, source: io::Error) -> StorageError {
    StorageError::after_dispatch(
        operation,
        crate::error::io_failure(&source),
        Some(Box::new(source)),
    )
}
