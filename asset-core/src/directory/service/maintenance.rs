//! Trusted storage-reconciliation operations for the Directory aggregate.

use super::DirectoryService;
use crate::{CoreError, directory::domain::DirectoryId};

/// Administrative Directory operations used after physical-storage reconciliation.
///
/// This handle shares the assembled [`DirectoryService`] mutation boundary and cannot create a
/// second store, lock, or filesystem configuration.
#[derive(Clone)]
pub struct DirectoryMaintenanceService {
    directory: DirectoryService,
}

impl DirectoryMaintenanceService {
    pub(super) fn new(directory: DirectoryService) -> Self {
        Self { directory }
    }

    /// Remove an empty non-root directory after a storage scan established its physical absence.
    ///
    /// The Directory store remains authoritative: its conditional delete checks emptiness and the
    /// current revision together while holding the shared Directory mutation lock.
    pub async fn delete_if_empty_after_storage_reconciliation(
        &self,
        id: &DirectoryId,
    ) -> Result<bool, CoreError> {
        self.directory.delete_if_empty_for_maintenance(id).await
    }
}
