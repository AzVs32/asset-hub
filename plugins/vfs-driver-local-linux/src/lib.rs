#![cfg(target_os = "linux")]
//! Linux implementation of the logical `local` driver, served over gRPC.
//! The plugin owns its database and filesystem handles. Deployment selects this
//! binary on Linux; the host uses the same protocol and identity on every OS.

mod config;
mod driver;
mod error;
mod storage;

pub use config::LinuxLocalDriverConfig;
pub use driver::LinuxLocalDriver;
pub use error::LinuxLocalDriverError;
pub use storage::LinuxLocalStorage;
