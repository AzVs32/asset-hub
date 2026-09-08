//! Durable execution leases for externally retried write commands.
//!
//! The SQLite record is the source of truth. A lease lets a later request recover after a process
//! crash, while a random execution ID prevents an earlier, stale executor from completing or
//! abandoning the newer executor's record. Long-running commands renew their lease while their
//! future is polled; a lease duration must still comfortably exceed ordinary scheduling delays.

use crate::CoreError;
use crate::idempotency::{
    domain::{IdempotencyExecutionId, IdempotencyKey, IdempotencyRecord},
    port::{IdempotencyAcquire, IdempotencyRepository},
};
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
pub struct IdempotencyService {
    repository: Arc<dyn IdempotencyRepository>,
    lease_duration: Duration,
    lease_chrono_duration: ChronoDuration,
}

/// Decision returned by [`IdempotencyService::begin`].
pub enum IdempotencyOutcome {
    /// No prior result: this execution owns the lease and may execute the command.
    Acquired {
        execution_id: IdempotencyExecutionId,
    },
    /// A completed, matching record exists: return its stored result without executing.
    Replay(serde_json::Value),
    /// The key was previously used by another logical request.
    ConflictDifferentRequest,
    /// The same logical request is currently being executed by a valid lease owner.
    AlreadyInProgress,
}

impl IdempotencyService {
    pub fn new(repository: Arc<dyn IdempotencyRepository>) -> Self {
        Self {
            repository,
            lease_duration: DEFAULT_LEASE_DURATION,
            lease_chrono_duration: ChronoDuration::seconds(5 * 60),
        }
    }

    pub fn with_lease_duration(
        repository: Arc<dyn IdempotencyRepository>,
        lease_duration: Duration,
    ) -> Result<Self, CoreError> {
        if lease_duration.is_zero() {
            return Err(CoreError::configuration(
                "idempotency lease duration must be greater than zero",
            ));
        }
        let lease_chrono_duration = ChronoDuration::from_std(lease_duration).map_err(|_| {
            CoreError::configuration("idempotency lease duration must be representable by chrono")
        })?;
        Ok(Self {
            repository,
            lease_duration,
            lease_chrono_duration,
        })
    }

    pub fn lease_duration(&self) -> Duration {
        self.lease_duration
    }

    pub async fn begin(
        &self,
        key: &IdempotencyKey,
        request_hash: &str,
    ) -> Result<IdempotencyOutcome, CoreError> {
        // A matching owner may abandon between a failed conditional acquire and the adapter's
        // subsequent read. Retry that narrow race; no process-local state decides ownership.
        loop {
            let now = Utc::now();
            let record = IdempotencyRecord::new(
                key.clone(),
                request_hash.to_string(),
                now + self.lease_chrono_duration,
            );
            match self.repository.acquire(&record).await? {
                IdempotencyAcquire::Acquired => {
                    return Ok(IdempotencyOutcome::Acquired {
                        execution_id: record.execution_id(),
                    });
                }
                IdempotencyAcquire::Existing(record) => return Ok(decide(&record, request_hash)),
                IdempotencyAcquire::Vacant => continue,
            }
        }
    }

    pub async fn complete(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
        result: serde_json::Value,
    ) -> Result<(), CoreError> {
        if self.repository.complete(key, execution_id, result).await? {
            Ok(())
        } else {
            Err(CoreError::lost_idempotency_lease(key.to_string()))
        }
    }

    pub async fn abandon(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
    ) -> Result<(), CoreError> {
        if self.repository.remove(key, execution_id).await? {
            Ok(())
        } else {
            Err(CoreError::lost_idempotency_lease(key.to_string()))
        }
    }

    /// Renew one execution's lease. This is also available to a caller that owns a workflow
    /// outside [`Self::execute_with_lease`].
    pub async fn renew(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
    ) -> Result<(), CoreError> {
        let expires_at = Utc::now() + self.lease_chrono_duration;
        if self.repository.renew(key, execution_id, expires_at).await? {
            Ok(())
        } else {
            Err(CoreError::lost_idempotency_lease(key.to_string()))
        }
    }

    /// Execute a command while periodically extending its persisted lease.
    ///
    /// This avoids treating a fixed TTL as proof that an otherwise live request has ended. A
    /// crashed process stops renewing, so a subsequent matching request can safely take over once
    /// the persisted expiry passes.
    pub async fn execute_with_lease<T, F>(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
        operation: F,
    ) -> Result<T, CoreError>
    where
        F: Future<Output = Result<T, CoreError>>,
    {
        let cadence = std::cmp::max(self.lease_duration / 2, Duration::from_millis(1));
        let mut renewals = tokio::time::interval(cadence);
        renewals.tick().await;
        let mut operation = std::pin::pin!(operation);
        loop {
            tokio::select! {
                result = &mut operation => return result,
                _ = renewals.tick() => self.renew(key, execution_id).await?,
            }
        }
    }
}

fn decide(record: &IdempotencyRecord, request_hash: &str) -> IdempotencyOutcome {
    if record.request_hash() != request_hash {
        return IdempotencyOutcome::ConflictDifferentRequest;
    }
    if record.is_completed() {
        return IdempotencyOutcome::Replay(
            record.result().cloned().unwrap_or(serde_json::Value::Null),
        );
    }
    IdempotencyOutcome::AlreadyInProgress
}

/// Canonical fingerprint of a command's idempotency-relevant fields.
///
/// `serde_json` serializes object keys in sorted order, so the same logical request always yields
/// the same hash. The hash excludes the idempotency key itself.
pub fn request_hash(canonical: &serde_json::Value) -> String {
    let json = serde_json::to_string(canonical).expect("idempotency fingerprint must serialize");
    let digest = Sha256::digest(json.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}
