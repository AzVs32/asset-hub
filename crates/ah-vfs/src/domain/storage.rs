//! Backend-independent values, requests and semantic guarantees for storage I/O.
//!
//! Addresses are relative to one configured storage instance. Entry metadata,
//! operation modes, directory pagination, capabilities, attributes and failures
//! share this contract without depending on virtual namespaces or application
//! services. Async storage handles and stream lifetimes are application ports.
//!
//! Capabilities declare support; validate them at acceptance boundaries rather
//! than inferring guarantees. Unsupported operations must fail before side effects.
//! Transport adapters preserve these semantics using their own wire formats.

mod attributes;
mod capabilities;
mod entry;
mod error;
mod operations;
mod pagination;
mod path;

pub use attributes::{EntryPermissions, XattrName, XattrWriteMode};
pub use capabilities::{
    AttributeSupport, DirModel, FileReadSupport, FileWriteSupport, RenameSupport,
    StorageCapabilities, StorageCapabilitiesError,
};
pub use entry::{StorageEntry, StorageEntryKind};
pub use error::{StorageError, StorageErrorKind};
pub use operations::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageMoveEntryRequest,
    StorageReadFileRequest, StorageRemoveEntryRequest, StorageStatEntryRequest,
    StorageWriteFileMode, StorageWriteFileRequest,
};
pub use pagination::{DEFAULT_DIR_PAGE_LIMIT, MAX_DIR_PAGE_LIMIT, validate_dir_page_limit};
pub use path::{StoragePath, StoragePathError};
