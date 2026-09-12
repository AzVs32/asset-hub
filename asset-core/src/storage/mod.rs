//! Storage capability boundary: canonical keys, internal namespace, and adapter ports.

mod key;
mod mutation;

pub mod port;

pub use key::{RESERVED_BLOB_STORAGE_PREFIX, StorageKey};
pub use mutation::StorageMutationCoordinator;
