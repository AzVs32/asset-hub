//! Transitional operation requests. Shared values belong to ah-vfs-storage.

mod operations;
mod pagination;

pub use operations::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageMoveEntryRequest,
    StorageReadFileRequest, StorageRemoveEntryRequest, StorageStatEntryRequest,
    StorageWriteFileRequest,
};
pub use pagination::{DEFAULT_DIR_PAGE_LIMIT, MAX_DIR_PAGE_LIMIT, validate_dir_page_limit};
