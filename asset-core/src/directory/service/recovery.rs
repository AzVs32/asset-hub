//! Process-lifecycle recovery for interrupted Directory relocations.

use super::DirectoryService;
use crate::CoreError;

/// Startup and operator recovery operations for the Directory aggregate.
///
/// This handle shares the assembled [`DirectoryService`] mutation boundary; constructing it does
/// not create a second store, lock, or filesystem configuration.
#[derive(Clone)]
pub struct DirectoryRecoveryService {
    directory: DirectoryService,
}

impl DirectoryRecoveryService {
    pub(super) fn new(directory: DirectoryService) -> Self {
        Self { directory }
    }

    /// Complete every durable Directory relocation left by an interrupted process.
    pub async fn recover_pending_relocations(&self) -> Result<u64, CoreError> {
        self.directory.recover_pending_relocations().await
    }
}
