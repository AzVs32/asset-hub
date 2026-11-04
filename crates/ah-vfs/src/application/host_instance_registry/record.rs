//! Validated host records; their persistence representation stays private.

use super::envelope::Envelope;
use crate::domain::{DriverError, DriverInstance, StorageError, StorageErrorKind};

/// One persisted host record. config_json is an opaque encrypted-parameters/public-
/// details envelope, never a plaintext secret object. Debug omits the envelope.
#[derive(Clone)]
pub struct InstanceRecord {
    instance: DriverInstance,
    config_version: u32,
    revision: u64,
    envelope: Envelope,
    created_at: u64,
    updated_at: u64,
}
impl InstanceRecord {
    /// Validate numeric ranges, envelope/ciphertext shape and timestamps on every
    /// load, including pagination lookahead records before they are truncated.
    pub fn new(
        instance: DriverInstance,
        config_version: u32,
        revision: u64,
        config_json: String,
        created_at: u64,
        updated_at: u64,
    ) -> Result<Self, DriverError> {
        Self::from_envelope(
            instance,
            config_version,
            revision,
            Envelope::parse(config_json)?,
            created_at,
            updated_at,
        )
    }

    pub(super) fn from_envelope(
        instance: DriverInstance,
        config_version: u32,
        revision: u64,
        envelope: Envelope,
        created_at: u64,
        updated_at: u64,
    ) -> Result<Self, DriverError> {
        if config_version == 0
            || revision == 0
            || revision > i64::MAX as u64
            || updated_at > i64::MAX as u64
            || created_at > updated_at
        {
            return Err(StorageError::new(StorageErrorKind::InvalidData).into());
        }
        Ok(Self {
            instance,
            config_version,
            revision,
            envelope,
            created_at,
            updated_at,
        })
    }
    pub(super) fn envelope(&self) -> &Envelope {
        &self.envelope
    }

    pub fn instance(&self) -> &DriverInstance {
        &self.instance
    }
    /// Configuration schema version, unrelated to the update revision.
    pub fn config_version(&self) -> u32 {
        self.config_version
    }
    /// Monotonically increasing optimistic-concurrency revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn config_json(&self) -> &str {
        self.envelope.json()
    }
    /// Creation time in milliseconds since the Unix epoch.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    /// Last successful update time in Unix milliseconds, including logical ticks.
    pub fn updated_at(&self) -> u64 {
        self.updated_at
    }
}
impl std::fmt::Debug for InstanceRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceRecord")
            .field("instance", &self.instance)
            .field("config_version", &self.config_version)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}
pub struct InstanceRecordPage {
    pub records: Vec<InstanceRecord>,
    pub next_after: Option<crate::domain::InstanceId>,
}
