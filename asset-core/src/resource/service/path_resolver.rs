//! Centralized derivation of physical storage keys.
//!
//! Every service that needs a logical Resource path or an internal staging/backup/deletion key
//! routes through these helpers so the reserved `.asset-hub` namespace layout has one source of
//! truth instead of being re-concatenated across `content`, `command`, and `upload`.

use crate::CoreError;
use crate::{
    directory::domain::DirectoryPath,
    resource::domain::{ResourceContentReplacementId, ResourceId, UploadId},
    storage::{RESERVED_BLOB_STORAGE_PREFIX, StorageKey},
};

/// Logical Resource path (Directory path + name) → Blob storage key.
pub(super) fn resource_key(directory: &DirectoryPath, name: &str) -> Result<StorageKey, CoreError> {
    super::super::storage_key_from_resource_path(directory, name).map_err(Into::into)
}

/// Internal staging key for a content replacement.
pub(super) fn replacement_staging_key(
    id: ResourceContentReplacementId,
) -> Result<StorageKey, CoreError> {
    Ok(StorageKey::new(format!(
        "{RESERVED_BLOB_STORAGE_PREFIX}/uploads/replacement-{id}"
    ))?)
}

/// Internal backup key for a content replacement.
pub(super) fn replacement_backup_key(
    id: ResourceContentReplacementId,
) -> Result<StorageKey, CoreError> {
    Ok(StorageKey::new(format!(
        "{RESERVED_BLOB_STORAGE_PREFIX}/content-backups/{id}"
    ))?)
}

/// Internal staging key for a streamed upload session.
pub(super) fn upload_staging_key(id: UploadId) -> Result<StorageKey, CoreError> {
    Ok(StorageKey::new(format!(
        "{RESERVED_BLOB_STORAGE_PREFIX}/uploads/{id}"
    ))?)
}

/// Internal chunk key for a streamed upload session.
pub(super) fn upload_chunk_key(id: UploadId) -> Result<StorageKey, CoreError> {
    Ok(StorageKey::new(format!(
        "{RESERVED_BLOB_STORAGE_PREFIX}/uploads/{id}.chunk"
    ))?)
}

/// Internal deletion-marker key used to stage a Resource Blob before its aggregate is removed.
pub(super) fn deletion_key(id: ResourceId) -> Result<StorageKey, CoreError> {
    Ok(StorageKey::new(format!(
        "{RESERVED_BLOB_STORAGE_PREFIX}/deletions/{id}"
    ))?)
}
