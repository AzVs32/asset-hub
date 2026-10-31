//! Extensible VFS configuration namespaces owned by the core module.

/// Parent namespace for VFS configuration declarations.
#[derive(ah_config::Config)]
#[config(key = "vfs", default)]
pub struct VfsConfig {}

/// Parent namespace extended independently by built-in and linked drivers.
///
/// Drivers register child sections with `parent = DriverConfigNamespace`; the core does
/// not enumerate driver-specific configuration fields.
#[derive(ah_config::Config)]
#[config(parent = VfsConfig, key = "driver", default)]
pub struct DriverConfigNamespace {}
