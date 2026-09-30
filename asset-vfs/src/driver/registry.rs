use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;

use crate::driver::{Driver, DriverError, DriverKind};
use crate::error::VfsError;

/// An in-memory mapping from driver kinds to shared driver factories.
///
/// DriverService registers implementations during construction and owns the
/// registry. Registration and lookup do not bind backends.
/// Each kind selects one factory; bound roots remain specific to each mount.
#[derive(Default)]
pub(crate) struct DriverRegistry {
    drivers: HashMap<DriverKind, Arc<dyn Driver>>,
}

impl DriverRegistry {
    /// Creates an empty registry.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Registers a factory under its own declared kind.
    ///
    /// A duplicate kind returns [`DriverError::DuplicateKind`] without replacing
    /// the previously registered factory or changing any other registration.
    pub(crate) fn register(&mut self, driver: Arc<dyn Driver>) -> Result<(), VfsError> {
        match self.drivers.entry(driver.kind()) {
            Entry::Vacant(entry) => {
                entry.insert(driver);
                Ok(())
            }
            Entry::Occupied(entry) => Err(DriverError::DuplicateKind(entry.key().clone()).into()),
        }
    }

    /// Returns the registered factory, or `None` for an unknown kind.
    pub(crate) fn get(&self, kind: &DriverKind) -> Option<&dyn Driver> {
        self.drivers.get(kind).map(|driver| driver.as_ref())
    }

    /// Returns the registered factory, without binding a backend.
    ///
    /// An unknown kind returns [`DriverError::UnregisteredKind`].
    pub(crate) fn require(&self, kind: &DriverKind) -> Result<&dyn Driver, VfsError> {
        self.get(kind)
            .ok_or_else(|| DriverError::UnregisteredKind(kind.clone()).into())
    }

    /// Returns the number of registered kinds, not the number of bound backends.
    pub(crate) fn len(&self) -> usize {
        self.drivers.len()
    }

    /// Returns all registered kinds in ascending order, without binding backends.
    ///
    /// Each kind appears once. The returned snapshot is independent of later
    /// registrations and does not expose the registry's internal storage.
    pub(crate) fn list(&self) -> Vec<DriverKind> {
        let mut kinds: Vec<_> = self.drivers.keys().cloned().collect();
        kinds.sort_unstable();
        kinds
    }
}

#[cfg(test)]
mod tests;
