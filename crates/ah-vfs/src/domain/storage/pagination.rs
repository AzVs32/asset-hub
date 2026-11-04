//! Shared bounds for live directory pagination.

use super::{StorageError, StorageErrorKind};
use std::num::NonZeroUsize;

/// Default maximum number of entries returned by a directory query.
pub const DEFAULT_DIR_PAGE_LIMIT: NonZeroUsize = match NonZeroUsize::new(100) {
    Some(limit) => limit,
    None => NonZeroUsize::MIN,
};

/// Largest supported directory page, also used for VFS backend scans.
pub const MAX_DIR_PAGE_LIMIT: NonZeroUsize = match NonZeroUsize::new(1000) {
    Some(limit) => limit,
    None => NonZeroUsize::MIN,
};

/// Reject oversized directory pages before accessing backend entries.
/// Zero is excluded by the input type; an oversized limit returns InvalidRequest.
pub fn validate_dir_page_limit(limit: NonZeroUsize) -> Result<(), StorageError> {
    if limit > MAX_DIR_PAGE_LIMIT {
        return Err(StorageError::new(StorageErrorKind::InvalidRequest));
    }
    Ok(())
}
