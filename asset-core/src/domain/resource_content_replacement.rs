//! Pending Resource content replacement state used for crash recovery.

use crate::error::ResourceError;

use super::{ResourceContent, ResourceId, StorageKey};

crate::gen_id_uuid_v7!(ResourceContentReplacementId);

/// Durable intent recorded before any content replacement mutates the public Blob path.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceContentReplacement {
    id: ResourceContentReplacementId,
    resource_id: ResourceId,
    expected_revision: u64,
    target_key: StorageKey,
    staged_key: StorageKey,
    backup_key: StorageKey,
    replacement_content: ResourceContent,
}

impl ResourceContentReplacement {
    pub fn new(
        resource_id: ResourceId,
        expected_revision: u64,
        target_key: StorageKey,
        staged_key: StorageKey,
        backup_key: StorageKey,
        replacement_content: ResourceContent,
    ) -> Result<Self, ResourceError> {
        Self::rehydrate(
            ResourceContentReplacementId::new(),
            resource_id,
            expected_revision,
            target_key,
            staged_key,
            backup_key,
            replacement_content,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: ResourceContentReplacementId,
        resource_id: ResourceId,
        expected_revision: u64,
        target_key: StorageKey,
        staged_key: StorageKey,
        backup_key: StorageKey,
        replacement_content: ResourceContent,
    ) -> Result<Self, ResourceError> {
        if expected_revision == 0 {
            return Err(ResourceError::InvalidFormat {
                field: "resource_content_replacement.expected_revision",
                reason: "expected revision must be greater than zero",
            });
        }
        Ok(Self {
            id,
            resource_id,
            expected_revision,
            target_key,
            staged_key,
            backup_key,
            replacement_content,
        })
    }

    pub fn id(&self) -> ResourceContentReplacementId {
        self.id
    }

    pub fn resource_id(&self) -> ResourceId {
        self.resource_id
    }

    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub fn target_key(&self) -> &StorageKey {
        &self.target_key
    }

    pub fn staged_key(&self) -> &StorageKey {
        &self.staged_key
    }

    pub fn backup_key(&self) -> &StorageKey {
        &self.backup_key
    }

    pub fn replacement_content(&self) -> &ResourceContent {
        &self.replacement_content
    }
}
