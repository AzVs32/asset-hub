//! Backend-independent VFS identities, paths, mount rules and storage semantics.
//!
//! [`MountTable`] owns namespace invariants. Mount definitions reference host-owned
//! instances; they do not own instance configuration or live storage handles.
//! Driver registration, persistence, authorization, asynchronous I/O and transport
//! formats belong outside this module. Nesting policies must come from one stable
//! driver-declaration snapshot during each table mutation or restoration.
//!
//! [`VirtualPath`] is a canonical namespace address; [`StoragePath`] is an
//! instance-local address with stricter character rules. Convert a routing result
//! explicitly with [`MountResolution::storage_path`]; a valid virtual name need not
//! be a valid backend address. Neither type provides native filesystem confinement.
//!
//! [`StorageCapabilities`] is an adapter declaration and must be validated before
//! acceptance. Future optional capabilities must default to unsupported; transport
//! adapters own version negotiation and must preserve existing semantic guarantees.
//! Domain types deliberately do not define a persistence or wire representation.

mod child_mount_policy;
mod driver_instance;
mod driver_key;
mod instance_id;
mod mount;
mod mount_id;
mod mount_table;
mod storage_path;
mod virtual_path;

pub use child_mount_policy::ChildMountPolicy;
pub use driver_instance::DriverInstance;
pub use driver_key::{DriverKey, DriverKeyError};
pub use instance_id::{InstanceId, InstanceIdError, new_instance_id};
pub use mount::Mount;
pub use mount_id::MountId;
pub use mount_table::{MountResolution, MountTable, MountTableError};
pub use storage_path::{StoragePath, StoragePathError};
pub use virtual_path::{VirtualPath, VirtualPathError};

mod storage_attributes;
mod storage_capabilities;
mod storage_error;
pub use storage_attributes::{EntryPermissions, XattrName, XattrWriteMode};
pub use storage_capabilities::{
    AttributeSupport, DirModel, FileReadSupport, FileWriteSupport, RenameSupport,
    StorageCapabilities, StorageCapabilitiesError,
};
pub use storage_error::{DriverError, InstanceConfigErrorKind, StorageError, StorageErrorKind};
