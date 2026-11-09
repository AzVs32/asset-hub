//! Errors returned by virtual filesystem application services.

use crate::application::driver_error::DriverError;
use crate::domain::namespace::{MountId, MountTableError, VirtualPath};
use crate::domain::{DriverInstance, DriverKey};
use ah_vfs_storage::StorageError;
use thiserror::Error;

/// Failures when changing driver registrations.
#[derive(Error)]
pub enum DriverCatalogError {
    #[error("driver '{0}' is already registered")]
    AlreadyRegistered(DriverKey),
    #[error("driver '{0}' is not registered")]
    NotRegistered(DriverKey),
    #[error("driver initialization failed")]
    Initialization(#[source] crate::application::driver_error::DriverError),
    #[error("driver declaration is incompatible with the host")]
    InvalidDeclaration(#[source] crate::application::driver_error::DriverError),
}

impl std::fmt::Debug for DriverCatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered(key) => f.debug_tuple("AlreadyRegistered").field(key).finish(),
            Self::NotRegistered(key) => f.debug_tuple("NotRegistered").field(key).finish(),
            Self::Initialization(_) => f
                .debug_tuple("Initialization")
                .field(&"[REDACTED]")
                .finish(),
            Self::InvalidDeclaration(_) => f
                .debug_tuple("InvalidDeclaration")
                .field(&"[REDACTED]")
                .finish(),
        }
    }
}

/// Failures when reading or writing mount definitions.
#[derive(Debug, Error)]
pub enum MountRepositoryError {
    #[error("mount '{0}' already exists")]
    DuplicateId(MountId),
    #[error("virtual path '{0}' is already occupied")]
    PathOccupied(VirtualPath),
    #[error("mount '{0}' was not found")]
    NotFound(MountId),
    #[error("stored mount data is invalid")]
    InvalidData(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("mount persistence failed")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Failures while managing or initializing mounts.
#[derive(Debug, Error)]
pub enum MountError {
    #[error(transparent)]
    Table(#[from] MountTableError),
    #[error(transparent)]
    Repository(#[from] MountRepositoryError),
    #[error("driver '{0}' is not registered")]
    DriverNotRegistered(DriverKey),
    #[error(transparent)]
    Driver(#[from] DriverError),
}

/// Driver-independent instance lookup and reference-protected removal failures.
#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("driver '{0}' is not registered")]
    DriverNotRegistered(DriverKey),
    #[error(transparent)]
    Repository(#[from] MountRepositoryError),
    #[error(transparent)]
    Driver(#[from] DriverError),
    #[error("driver instance is still referenced by mounts")]
    InstanceInUse {
        instance: DriverInstance,
        mounts: Vec<MountId>,
    },
}

impl InstanceError {
    pub fn lifecycle_error(&self) -> Option<&crate::application::driver_error::LifecycleError> {
        match self {
            Self::Driver(error) => error.lifecycle_error(),
            _ => None,
        }
    }
}

/// Namespace routing, mount-boundary and backend access failures.
#[derive(Debug, Error)]
pub enum EntryError {
    #[error(transparent)]
    InvalidReadRange(#[from] ah_vfs_storage::RangeError),
    #[error(transparent)]
    Table(#[from] MountTableError),
    #[error(transparent)]
    Repository(#[from] MountRepositoryError),
    #[error("driver '{0}' is not registered")]
    DriverNotRegistered(DriverKey),
    #[error(transparent)]
    Driver(#[from] DriverError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    InvalidPath(#[from] crate::domain::namespace::VirtualPathError),
    #[error(transparent)]
    InvalidStorageAddress(#[from] ah_vfs_storage::PathError),
    #[error("'{0}' is a mount point or contains mounts; entry operations cannot modify it")]
    MountBoundary(VirtualPath),
    #[error("moving entries between different mounts is not supported")]
    CrossMountMove,
    #[error("invalid storage response: {0}")]
    InvalidStorageResponse(&'static str),
}

impl EntryError {
    /// Inspect the domain storage failure from I/O.
    /// Namespace and mount failures have no backend storage classification.
    pub fn storage_error(&self) -> Option<&StorageError> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Driver(error) => error.storage_error(),
            _ => None,
        }
    }
}
impl MountError {
    /// Preserve normalized instance-opening failures through mount workflows.
    pub fn lifecycle_error(&self) -> Option<&crate::application::driver_error::LifecycleError> {
        match self {
            Self::Driver(error) => error.lifecycle_error(),
            _ => None,
        }
    }
}
