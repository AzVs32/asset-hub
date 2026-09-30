use std::sync::Arc;

use crate::error::VfsError;
use crate::mount::{Mount, MountId, MountRepository};

/// Public entry point for managing mounts.
///
/// Queries read the current persisted definitions, including disabled mounts.
/// They do not bind drivers or check backend availability. Mutation and
/// path-resolution methods remain API placeholders.
pub struct MountService {
    repository: Arc<dyn MountRepository>,
}

impl MountService {
    /// Creates a service using the shared mount repository.
    pub fn new(repository: Arc<dyn MountRepository>) -> Self {
        Self { repository }
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
    /// Includes disabled mounts. Repository errors are returned unchanged.
    pub async fn list_mounts(&self) -> Result<Vec<Mount>, VfsError> {
        self.repository.list().await
    }

    /// Returns information about one mount.
    ///
    /// Returns `None` for an unknown ID, and includes disabled mounts.
    /// Repository errors are returned unchanged.
    pub async fn mount_info(&self, id: MountId) -> Result<Option<Mount>, VfsError> {
        self.repository.get(id).await
    }

    /// Resolves a virtual path to its covering mount.
    pub async fn resolve_mount(&self) {
        todo!("MountService::resolve_mount")
    }
}
