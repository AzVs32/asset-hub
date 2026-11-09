//! Public instance views and driver-scoped live pagination.

use crate::application::driver_error::DriverError;
use crate::application::driver_error::{LifecycleError, LifecycleFailure};
use crate::domain::InstanceId;
use std::{collections::BTreeMap, num::NonZeroUsize};

/// Public information about one registered instance, not a live storage handle.
///
/// Identity is scoped to the queried driver; combine its key with `id` when
/// aggregating results. Details are a curated JSON object owned by that driver,
/// such as a local root or memory quota. They must exclude secrets and are not
/// a provisioning configuration, a health check or a storage capability report.
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceInfo {
    pub id: InstanceId,
    pub details: BTreeMap<String, serde_json::Value>,
}

/// A bounded page of instance records ordered by ascending UTF-8 ID bytes.
///
/// `after` is an exclusive ID boundary, not an offset or a backend token. It need
/// not exist. Queries belong to one driver and are live, without cross-page
/// snapshot isolation. Deleting the cursor record does not invalidate the boundary.
#[derive(Clone, Debug)]
pub struct InstanceQuery {
    after: Option<InstanceId>,
    limit: NonZeroUsize,
}

impl InstanceQuery {
    pub const MAX_LIMIT: usize = 1000;

    /// Reject oversized pages with the domain InvalidRequest classification.
    /// Zero cannot be represented by the input type.
    pub fn new(limit: NonZeroUsize) -> Result<Self, DriverError> {
        if limit.get() > Self::MAX_LIMIT {
            return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into());
        }
        Ok(Self { after: None, limit })
    }

    pub fn with_after(mut self, after: InstanceId) -> Self {
        self.after = Some(after);
        self
    }

    pub fn after(&self) -> Option<&InstanceId> {
        self.after.as_ref()
    }

    pub fn limit(&self) -> NonZeroUsize {
        self.limit
    }
}

impl Default for InstanceQuery {
    /// Start before the first record with a maximum of 100 instances.
    fn default() -> Self {
        Self {
            after: None,
            limit: match NonZeroUsize::new(100) {
                Some(limit) => limit,
                None => NonZeroUsize::MIN,
            },
        }
    }
}

/// A live page containing at most the requested limit, with distinct sorted IDs.
#[derive(Clone, Debug, PartialEq)]
pub struct InstancePage {
    pub instances: Vec<InstanceInfo>,
    /// Last returned ID, only when more records exist now. Continue with
    /// `InstanceQuery::with_after`; `None` means the current scan has ended.
    pub next_after: Option<InstanceId>,
}

/// Confirmed absence after removing an instance registration. Neither outcome
/// reports the lifetime of previously opened storage or a data-erasure operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoveInstanceOutcome {
    /// An existing instance registration was removed.
    Removed,
    /// The instance registration was already absent when removal was processed.
    AlreadyAbsent,
}
