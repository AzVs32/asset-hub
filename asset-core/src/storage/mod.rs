//! Storage capability boundary: canonical keys, internal namespace, and adapter ports.

mod key;

pub mod port;

pub use key::{RESERVED_BLOB_STORAGE_PREFIX, StorageKey};
