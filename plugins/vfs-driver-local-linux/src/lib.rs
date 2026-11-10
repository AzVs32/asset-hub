#![cfg(target_os = "linux")]
//! Linux implementation of the logical `local` driver, served over gRPC.
//! The host owns instance records; the plugin owns filesystem handles. Deployment selects this
//! binary on Linux; the host uses the same protocol and identity on every OS.
//!
//! Staged writes can create files in the instance root. Publishing into a nested
//! directory or replacing an existing file would require an atomic check of a
//! directory or destination identity that the native publication call cannot make.
//! Rename can move into an empty slot and recognize a same-slot request. It
//! rejects replacement of a distinct occupied slot for the same reason.
//!
//! Staging is anonymous. When upgrading an instance that used named stages,
//! remove abandoned `.asset-hub-write-` staging files from its root tree before
//! opening it with this driver. Existing names with that prefix are now visible.

mod driver;
mod error;
mod storage;

pub use driver::LinuxLocalDriver;
pub use error::LinuxLocalDriverError;
pub use storage::LinuxLocalStorage;
