//! Pure VFS identities, namespace rules and backend-independent storage contracts.
//!
//! [`namespace`] maps virtual mount points to shared [`DriverInstance`] identities.
//! [`storage`] defines instance-local addresses, values, operations and guarantees.
//! Namespace resolution may validate a routed suffix as a storage address; storage
//! semantics do not depend on virtual paths or mounts.
//!
//! Driver identities and lifecycle errors are shared by host instance management
//! and mounting. Application services, asynchronous ports, persistence and plugin
//! wire representations belong outside the domain.

pub mod namespace;
pub mod storage;

mod driver_error;
mod driver_instance;
mod driver_key;
mod instance_id;

pub use driver_error::{DriverError, InstanceConfigErrorKind};
pub use driver_instance::DriverInstance;
pub use driver_key::{DriverKey, DriverKeyError};
pub use instance_id::{InstanceId, InstanceIdError, new_instance_id};
