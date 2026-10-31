//! Host-owned versioned instance persistence, separate from mount definitions.

use super::{DriverFuture, InstanceRecord, InstanceRecordPage};
use crate::{
    application::dto::{InstanceQuery, RemoveInstanceOutcome},
    domain::{DriverInstanceRef, DriverKey},
};

/// The host stores records by (driver key, instance ID). Reads never contact backend
/// storage. Invalid records fail the entire fetched page. remove bypasses config
/// decoding so corrupt/unavailable backends do not prevent unregistering records.
pub trait InstanceRecordRepository: Send + Sync {
    fn get(&self, instance: DriverInstanceRef) -> DriverFuture<'_, Option<InstanceRecord>>;
    fn list(&self, driver: DriverKey, query: InstanceQuery)
    -> DriverFuture<'_, InstanceRecordPage>;
    fn insert(&self, record: InstanceRecord) -> DriverFuture<'_, ()>;
    /// Atomically replace only the expected revision; mismatch is Conflict, never
    /// an upsert. Revision/timestamp increments are prepared by the host registry.
    fn update(&self, record: InstanceRecord, expected_revision: u64) -> DriverFuture<'_, ()>;
    fn remove(&self, instance: DriverInstanceRef) -> DriverFuture<'_, RemoveInstanceOutcome>;
}
