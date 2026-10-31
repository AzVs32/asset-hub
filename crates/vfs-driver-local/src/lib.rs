//! Local-directory instances, driver configuration and persistence for Asset Hub VFS.
//!
//! This adapter depends on ah-vfs ports and configuration namespaces. The host
//! supplies its database pool and registers the driver; the VFS core does not
//! depend on this crate. Storage operations use directory capabilities and
//! staged complete-file writes with explicit commit or abort.

mod config;
mod driver;
mod error;
mod storage;

pub use config::LocalDriverConfig;
pub use driver::LocalDriver;
pub use error::LocalDriverError;
pub use storage::LocalStorage;
