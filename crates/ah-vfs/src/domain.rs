//! VFS identities and virtual namespace rules.
//!
//! [`namespace`] maps virtual mount points to shared [`DriverInstance`] identities.
//! [`ah_vfs_storage`] defines instance-local addresses, values, operations and
//! guarantees. Namespace resolution maps a routed suffix to a storage address.
//!
//! Driver identities and lifecycle errors are shared by host instance management
//! and mounting. Application services, asynchronous ports, persistence and plugin
//! wire representations belong outside the domain.

pub mod namespace;

mod driver_instance;
mod driver_key;
mod instance_id;

pub use driver_instance::DriverInstance;
pub use driver_key::{DriverKey, DriverKeyError};
pub use instance_id::{InstanceId, InstanceIdError};
