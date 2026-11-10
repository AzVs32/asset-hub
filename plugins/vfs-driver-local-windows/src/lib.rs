#![cfg(target_os = "windows")]
//! Windows implementation of the logical `local` driver, served over gRPC.
//! The host owns instance records; the plugin owns filesystem handles. Deployment selects this
//! binary on Windows; the host uses the same protocol and identity on every OS.
//! Windows names/reparse points and handle-based rename/deletion are handled
//! here; POSIX permissions and xattrs are deliberately unsupported.
//!
//! Staged writes can create files. The native publish operation can require an
//! empty destination slot, but cannot condition replacement on a file identity.
//! Rename can move into an empty slot and recognize a same-slot request. It
//! rejects replacement of a distinct occupied slot for the same reason.

mod driver;
mod error;
mod storage;

pub use driver::WindowsLocalDriver;
pub use error::WindowsLocalDriverError;
pub use storage::WindowsLocalStorage;
