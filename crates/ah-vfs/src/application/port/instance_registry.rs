//! Driver-scoped instance persistence, independent of backend storage opening.

use super::DriverFuture;
use crate::application::dto::{
    InstanceConfig, InstanceInfo, InstancePage, InstanceQuery, RemoveInstanceOutcome,
};
use crate::domain::InstanceId;
use std::sync::Arc;

/// Persistent instance records of one driver. Queries never open storage.
/// IDs use exclusive, ascending UTF-8 cursor boundaries, even for missing or
/// deleted cursor records. Invalid fetched records fail the entire page.
pub trait InstanceRegistry: Send + Sync {
    /// Missing records return None. Persistence/transport failures remain errors;
    /// backend unavailability never means that a registration disappeared.
    fn get(&self, id: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>>;
    /// Return a bounded live page. Empty pages mean no matching records remain;
    /// next_after is the last returned ID only when further records exist.
    fn list(&self, query: InstanceQuery) -> DriverFuture<'_, InstancePage>;
    /// Validate parameters before persistence and allocate a driver-scoped ID.
    /// Invalid input uses InvalidInstanceConfig. Public details exclude secrets.
    /// Creation may not be idempotent: reconcile unknown/cancelled outcomes before
    /// retrying. Never open storage as part of record creation.
    fn create(&self, config: InstanceConfig) -> DriverFuture<'_, InstanceInfo>;
    /// Remove registration and driver-owned runtime retention without actively
    /// clearing backend contents. Repetition returns AlreadyAbsent; get/list then
    /// exclude the ID and new opens fail with InstanceNotFound. Existing sessions
    /// retain their resources. Never open storage or require valid backend settings
    /// to remove records. Unsupported removal fails before side effects; uncertain
    /// outcomes use OutcomeUnknown. Mount-reference checks belong to the host.
    fn remove(&self, id: InstanceId) -> DriverFuture<'_, RemoveInstanceOutcome>;
}

/// Ownership of one driver's instance records. Self-managed registries may use
/// any database or transport; the host only depends on their object-safe port.
pub enum InstanceManagement {
    SelfManaged(Arc<dyn InstanceRegistry>),
    /// Stateless preparation; record persistence and secrets belong to the host.
    HostManaged(Arc<dyn super::InstanceProvisioner>),
}

impl InstanceManagement {
    /// Access the self-managed port without opening storage. HostManaged returns
    /// UnsupportedOperation because its registry requires host-owned resources.
    pub fn registry(self) -> Result<Arc<dyn InstanceRegistry>, crate::domain::DriverError> {
        match self {
            Self::SelfManaged(registry) => Ok(registry),
            Self::HostManaged(_) => Err(crate::domain::StorageError::new(
                crate::domain::StorageErrorKind::UnsupportedOperation,
            )
            .into()),
        }
    }
}
