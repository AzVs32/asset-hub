//! Backend-independent values, requests and semantic guarantees for storage I/O.
//!
//! Addresses are relative to one configured storage instance. Entry metadata,
//! operation modes, directory pagination, capabilities, attributes and failures
//! share this contract without depending on virtual namespaces or application
//! services. Async storage handles and stream lifetimes are application ports.
//!
//! Capability construction rejects inconsistent declarations. Runtime acceptance
//! checks independent attribute interfaces against the driver declaration. All
//! instances share it; instance/path limitations are reported by operations.
//! Unsupported operations fail before side effects.
//! Transport adapters preserve these semantics using their own wire formats.

mod attributes;
mod capabilities;
mod entry;
mod error;
mod operations;
mod pagination;
mod path;

pub use attributes::{
    EntryPermissions, EntryPermissionsError, XattrName, XattrNameError, XattrWriteMode,
};
pub use capabilities::{
    AttributeSupport, DirModel, FileReadSupport, FileWriteSupport, RenameSupport,
    StorageCapabilities, StorageCapabilitiesBuilder, StorageCapabilitiesError,
};
pub use entry::{Entry, EntryKind};
pub use error::{StorageError, StorageErrorKind};
pub use operations::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageMoveEntryRequest,
    StorageReadFileRequest, StorageRemoveEntryRequest, StorageStatEntryRequest,
    StorageWriteFileMode, StorageWriteFileRequest,
};
pub use pagination::{DEFAULT_DIR_PAGE_LIMIT, MAX_DIR_PAGE_LIMIT, validate_dir_page_limit};
pub use path::{StoragePath, StoragePathError};
