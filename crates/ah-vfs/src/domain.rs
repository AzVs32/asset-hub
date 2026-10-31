//! Mount entities and platform-independent virtual paths.

mod child_mount_policy;
mod driver_instance_ref;
mod driver_key;
mod instance_id;
mod mount;
mod mount_id;
mod mount_table;
mod storage_path;
mod virtual_path;

pub use child_mount_policy::ChildMountPolicy;
pub use driver_instance_ref::DriverInstanceRef;
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
    StorageCapabilities,
};
pub use storage_error::{DriverError, InstanceConfigErrorKind, StorageError, StorageErrorKind};
