#![cfg(target_os = "linux")]
//! Linux implementation of the logical `local` driver, served over gRPC.
//! The host owns instance records; the plugin owns filesystem handles. Deployment selects this
//! binary on Linux; the host uses the same protocol and identity on every OS.

mod driver;
mod error;
mod storage;

pub use driver::LinuxLocalDriver;
pub use error::LinuxLocalDriverError;
pub use storage::LinuxLocalStorage;
