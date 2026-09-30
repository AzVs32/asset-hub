use std::sync::Arc;

use crate::DriverService;
use crate::error::VfsError;
use crate::mount::{Mount, MountId, MountInfo, MountRepository};

/// Public entry point for managing mounts.
///
/// Queries read the current persisted definitions, including disabled mounts.
/// Driver declarations come from the shared DriverService without binding backends
/// or checking backend availability. Mutation and path-resolution methods
/// remain API placeholders.
pub struct MountService {
    repository: Arc<dyn MountRepository>,
    drivers: Arc<DriverService>,
}

impl MountService {
    /// Creates a service using the mount repository and shared driver service.
    pub fn new(repository: Arc<dyn MountRepository>, drivers: Arc<DriverService>) -> Self {
        Self {
            repository,
            drivers,
        }
    }

    /// Mounts a backend in the virtual namespace.
    pub async fn mount(&self) {
        todo!("MountService::mount")
    }

    /// Removes a mount from the virtual namespace.
    pub async fn unmount(&self) {
        todo!("MountService::unmount")
    }

    /// Lists all configured mounts in ascending virtual-path order.
    ///
    /// Includes disabled mounts and their driver declarations. Unregistered
    /// kinds have unknown declarations. Repository errors are returned unchanged.
    pub async fn list_mounts(&self) -> Result<Vec<MountInfo>, VfsError> {
        Ok(self
            .repository
            .list()
            .await?
            .into_iter()
            .map(|mount| self.mount_info_from_definition(mount))
            .collect())
    }

    /// Returns information about one mount.
    ///
    /// Returns `None` for an unknown ID, and includes disabled mounts and their
    /// driver declarations. Unregistered kinds have unknown declarations.
    /// Repository errors are returned unchanged. Drivers are not bound.
    pub async fn mount_info(&self, id: MountId) -> Result<Option<MountInfo>, VfsError> {
        Ok(self
            .repository
            .get(id)
            .await?
            .map(|mount| self.mount_info_from_definition(mount)))
    }

    fn mount_info_from_definition(&self, mount: Mount) -> MountInfo {
        let allows_submounts = self
            .drivers
            .info(mount.driver())
            .map(|driver| driver.allows_submounts());
        MountInfo::new(mount, allows_submounts)
    }

    /// Resolves a virtual path to its covering mount.
    pub async fn resolve_mount(&self) {
        todo!("MountService::resolve_mount")
    }
}
