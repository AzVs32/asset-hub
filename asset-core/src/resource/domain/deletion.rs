//! Durable intent for a permanent Resource deletion that crosses database and Blob storage.

use crate::CoreError;
use crate::resource::domain::ResourceId;
use crate::storage::StorageKey;

/// A pending permanent deletion.
///
/// The intent is written before the visible Blob moves into the internal deletion namespace. It
/// deliberately outlives the Resource row so recovery can clean the staged Blob after a committed
/// database deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDeletion {
    resource_id: ResourceId,
    expected_revision: u64,
    source_key: StorageKey,
    deletion_key: StorageKey,
}

impl ResourceDeletion {
    pub fn new(
        resource_id: ResourceId,
        expected_revision: u64,
        source_key: StorageKey,
        deletion_key: StorageKey,
    ) -> Result<Self, CoreError> {
        if expected_revision == 0 {
            return Err(CoreError::invariant(
                "resource deletion expected revision must be greater than zero",
            ));
        }
        if source_key == deletion_key {
            return Err(CoreError::invariant(
                "resource deletion source and staging keys must differ",
            ));
        }
        Ok(Self {
            resource_id,
            expected_revision,
            source_key,
            deletion_key,
        })
    }

    pub fn resource_id(&self) -> ResourceId {
        self.resource_id
    }

    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub fn source_key(&self) -> &StorageKey {
        &self.source_key
    }

    pub fn deletion_key(&self) -> &StorageKey {
        &self.deletion_key
    }
}
