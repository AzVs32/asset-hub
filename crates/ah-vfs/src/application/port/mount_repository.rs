//! Persistence of mount definitions.

use crate::application::dto::MountFilter;
use crate::application::error::MountRepositoryError;
use crate::domain::{Mount, MountId};
use std::{future::Future, pin::Pin};

pub type MountRepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MountRepositoryError>> + Send + 'a>>;

/// Persistence boundary for one mount namespace, independent of database technology.
///
/// Mount constraints are validated by the domain before writes. Driver instance
/// configuration is outside this boundary. Concurrent changes are not coordinated.
pub trait MountRepository: Send + Sync {
    /// Retrieve a definition by ID, returning `None` when it does not exist.
    fn get(&self, id: MountId) -> MountRepositoryFuture<'_, Option<Mount>>;

    /// Query definitions using all supplied criteria. With no criteria, return both
    /// enabled and disabled mounts.
    ///
    /// An empty result with no filter represents an uninitialized namespace.
    fn list(&self, filter: MountFilter) -> MountRepositoryFuture<'_, Vec<Mount>>;

    /// Insert a definition, rejecting an existing ID or occupied virtual path.
    fn insert(&self, mount: Mount) -> MountRepositoryFuture<'_, ()>;

    /// Remove a definition, returning `NotFound` when its ID does not exist.
    fn remove(&self, id: MountId) -> MountRepositoryFuture<'_, ()>;

    /// Persist the enabled state, returning `NotFound` when the ID does not exist.
    fn set_enabled(&self, id: MountId, enabled: bool) -> MountRepositoryFuture<'_, ()>;
}
