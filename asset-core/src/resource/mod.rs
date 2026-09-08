//! Resource aggregate boundary: domain types, persistence ports, and application services.

pub mod domain;
pub mod port;
pub mod query;
pub mod service;

use crate::{directory::domain::DirectoryPath, error::StorageError, storage::StorageKey};

/// Derive the visible Blob key for a Resource name in a resolved Directory path.
///
/// Storage owns key validation; Resource owns this mapping from its logical directory and name to
/// a user-visible Blob location.
pub(in crate::resource) fn storage_key_from_resource_path(
    directory: &DirectoryPath,
    name: &str,
) -> Result<StorageKey, StorageError> {
    let value = if directory.is_root() {
        name.to_owned()
    } else {
        format!("{}/{}", directory.path(), name)
    };
    StorageKey::new(value)
}

#[cfg(test)]
mod storage_key_tests {
    use super::*;

    #[test]
    fn derives_visible_blob_keys_from_root_and_nested_resource_paths() {
        assert_eq!(
            storage_key_from_resource_path(&DirectoryPath::root(), "note.txt")
                .unwrap()
                .as_str(),
            "note.txt"
        );
        assert_eq!(
            storage_key_from_resource_path(
                &DirectoryPath::from_path("documents/reports").unwrap(),
                "note.txt"
            )
            .unwrap()
            .as_str(),
            "documents/reports/note.txt"
        );
    }
}
