#![cfg(target_os = "windows")]
//! Windows implementation of the logical `local` driver, served over gRPC.
//! The host owns instance records; the plugin owns filesystem handles. Deployment selects this
//! binary on Windows; the host uses the same protocol and identity on every OS.
//! Windows names/reparse points and handle-based rename/deletion are handled
//! here; POSIX permissions and xattrs are deliberately unsupported.

mod driver;
mod error;
mod storage;

pub use driver::WindowsLocalDriver;
pub use error::WindowsLocalDriverError;
pub use storage::WindowsLocalStorage;
