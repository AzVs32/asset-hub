use std::io::Read;

use crate::driver::error::DriverError;
use crate::entry::domain::{Entry, Metadata};
use crate::namespace::domain::{DriverPath, VirtualRelativePath};

/// A driver factory that validates and binds one driver-specific mount root.
pub trait Driver: Send + Sync {
    /// Creates a backend bound to `root`.
    ///
    /// Driver-specific configuration and path validation happen once here;
    /// subsequent operations are relative to the bound root.
    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, DriverError>;
}

/// A read-only backend bound to one driver-specific root.
pub trait BoundDriver: Send + Sync {
    /// Returns metadata for a path relative to the bound root.
    fn stat(&self, path: &VirtualRelativePath) -> Result<Metadata, DriverError>;

    /// Lists the direct children of a directory relative to the bound root.
    ///
    /// Every returned entry name must be valid in the virtual namespace and
    /// reusable in subsequent `stat`, `list`, or `read` calls.
    fn list(&self, path: &VirtualRelativePath) -> Result<Vec<Entry>, DriverError>;

    /// Opens a file relative to the bound root for streaming reads.
    fn read(
        &self,
        path: &VirtualRelativePath,
    ) -> Result<Box<dyn Read + Send + 'static>, DriverError>;
}
