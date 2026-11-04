//! Virtual paths, mount definitions and namespace routing invariants.
//!
//! [`MountTable`] owns the required root, unique mount identities and locations,
//! nesting policies and disabled boundaries. Mounts reference shared driver
//! instance identities without owning configurations or live storage handles.
//! Driver nesting policies must come from one stable declaration snapshot during
//! each table mutation or restoration.
//!
//! [`VirtualPath`] is an absolute namespace address. Routing borrows its suffix;
//! [`MountResolution::storage_path`] validates only that suffix against the
//! instance-local storage rules. Neither address type provides OS confinement.

mod child_mount_policy;
mod mount;
mod mount_id;
mod mount_table;
mod virtual_path;

pub use child_mount_policy::ChildMountPolicy;
pub use mount::Mount;
pub use mount_id::MountId;
pub use mount_table::{MountResolution, MountTable, MountTableError};
pub use virtual_path::{VirtualPath, VirtualPathError};
