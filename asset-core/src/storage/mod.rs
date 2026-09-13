//! Storage capability boundary: canonical keys, internal namespace, and adapter ports.

mod key;
mod mutation;

pub mod port;

pub use key::StorageKey;
pub(crate) use key::is_internal;
pub use mutation::StorageMutationCoordinator;
