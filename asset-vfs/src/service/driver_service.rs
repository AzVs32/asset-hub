use std::sync::Arc;

use crate::driver::{Driver, DriverInfo, DriverKind, DriverRegistry};
use crate::error::VfsError;

/// Public entry point for querying registered driver kinds and declarations.
///
/// The runtime supplies driver factories during construction. Each service owns
/// its registry; share this service through an `Arc` to use the same drivers in
/// other services. Queries do not bind backends or expose driver factories, and
/// registrations cannot be changed after construction.
///
/// The registry is an internal implementation detail:
///
/// ```compile_fail
/// use asset_vfs::driver::DriverRegistry;
/// ```
pub struct DriverService {
    drivers: DriverRegistry,
}

impl DriverService {
    /// Registers the supplied factories under their declared kinds.
    ///
    /// Duplicate kinds return [`crate::driver::DriverError::DuplicateKind`].
    /// Construction does not bind any backend or access persistent storage.
    pub fn new(drivers: impl IntoIterator<Item = Arc<dyn Driver>>) -> Result<Self, VfsError> {
        let mut registry = DriverRegistry::new();
        for driver in drivers {
            registry.register(driver)?;
        }
        Ok(Self { drivers: registry })
    }

    /// Returns an owned snapshot of registered kinds in ascending order.
    pub fn list(&self) -> Vec<DriverKind> {
        self.drivers.list()
    }

    /// Returns declarations for a registered kind, or `None` for an unknown kind.
    pub fn info(&self, kind: &DriverKind) -> Option<DriverInfo> {
        self.drivers
            .get(kind)
            .map(|driver| DriverInfo::new(kind.clone(), driver.allows_submounts()))
    }

    /// Returns declarations, or [`crate::driver::DriverError::UnregisteredKind`].
    ///
    /// This returns metadata; the internal registry's factory lookup is private.
    pub fn require(&self, kind: &DriverKind) -> Result<DriverInfo, VfsError> {
        let driver = self.drivers.require(kind)?;
        Ok(DriverInfo::new(kind.clone(), driver.allows_submounts()))
    }

    /// Returns the number of registered kinds.
    pub fn len(&self) -> usize {
        self.drivers.len()
    }

    /// Returns whether no driver kinds are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
