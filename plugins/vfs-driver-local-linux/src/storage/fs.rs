//! Relative directory traversal and backend-neutral error/metadata conversion.

use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use ah_vfs_storage::{Entry, EntryName, EntryType, StoragePath};

use cap_std::fs::{Dir, Metadata};
use std::io;

pub(super) fn error(operation: Operation, error: io::Error) -> StorageError {
    StorageError::without_effects(
        operation,
        crate::error::io_failure(&error),
        Some(Box::new(error)),
    )
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
            Failure::Contract(ContractViolation::InvalidEntry),
            Some(Box::new(cause)),
        )
    })
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
    bound: &ah_vfs_storage::BoundListRequest,
) -> Result<ah_vfs_storage::DirectoryPage, StorageError> {
    let operation = Operation::List;
    let address = bound.selection().address();
    let mut names = std::collections::BTreeSet::new();
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
            let metadata = dir
                .symlink_metadata(name.as_str())
                .map_err(|cause| error(operation, cause))?;
            entry(
                operation,
                address
                    .join(&name)
                    .map_err(|cause| list_path_error(operation, cause))?,
                metadata,
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
