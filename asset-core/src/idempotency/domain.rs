//! Durable business idempotency for externally retried write commands.
//!
//! Idempotency is orthogonal to optimistic concurrency (CAS), compensation, and crash recovery:
//! a relocation or content-replacement intent records *how to finish or roll back* an interrupted
//! write, while an idempotency record answers *whether this exact command was already applied* and
//! replays its result. They are never collapsed into one operation journal.

use crate::IdempotencyError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum accepted length for a client-supplied idempotency key.
pub const MAX_IDEMPOTENCY_KEY_LEN: usize = 200;

/// Client-supplied identity for one retryable write command.
///
/// The persisted schema currently has one global key namespace: the key is the sole record
/// identity across every command type. Clients must therefore make keys unique across operations
/// (for example, use an operation-specific prefix). Reusing a key for a different request is a
/// durable conflict, not an implicit per-operation namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(value: impl Into<String>) -> Result<Self, IdempotencyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(IdempotencyError::InvalidFormat {
                field: "idempotency.key",
                reason: "idempotency key must not be empty",
            });
        }
        if value.len() > MAX_IDEMPOTENCY_KEY_LEN {
            return Err(IdempotencyError::InvalidFormat {
                field: "idempotency.key",
                reason: "idempotency key exceeds the maximum length",
            });
        }
        if value.chars().any(char::is_control) {
            return Err(IdempotencyError::InvalidFormat {
                field: "idempotency.key",
                reason: "idempotency key must not contain control characters",
            });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for IdempotencyKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::str::FromStr for IdempotencyKey {
    type Err = IdempotencyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdempotencyStatus {
    InProgress,
    Completed,
}

/// Opaque identity for one concrete execution attempt of an idempotent command.
///
/// A retry that takes over an expired lease always receives a new value. Completion, abandonment,
/// and lease renewal must present this value so an old executor cannot mutate a newer attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyExecutionId(Uuid);

impl IdempotencyExecutionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

impl Default for IdempotencyExecutionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for IdempotencyExecutionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One durable idempotency record for a specific [`IdempotencyKey`].
///
/// `request_hash` fingerprints the command it guards; a replay with the same hash returns the
/// stored result, while a different hash under the same key is a conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    key: IdempotencyKey,
    request_hash: String,
    status: IdempotencyStatus,
    execution_id: IdempotencyExecutionId,
    lease_expires_at: DateTime<Utc>,
    result: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl IdempotencyRecord {
    pub fn new(key: IdempotencyKey, request_hash: String, lease_expires_at: DateTime<Utc>) -> Self {
        let now = Utc::now();
        Self {
            key,
            request_hash,
            status: IdempotencyStatus::InProgress,
            execution_id: IdempotencyExecutionId::new(),
            lease_expires_at,
            result: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        key: IdempotencyKey,
        request_hash: String,
        status: IdempotencyStatus,
        execution_id: IdempotencyExecutionId,
        lease_expires_at: DateTime<Utc>,
        result: Option<serde_json::Value>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            key,
            request_hash,
            status,
            execution_id,
            lease_expires_at,
            result,
            created_at,
            updated_at,
            completed_at,
        }
    }

    pub fn key(&self) -> &IdempotencyKey {
        &self.key
    }

    pub fn request_hash(&self) -> &str {
        &self.request_hash
    }

    pub fn status(&self) -> IdempotencyStatus {
        self.status
    }

    pub fn execution_id(&self) -> IdempotencyExecutionId {
        self.execution_id
    }

    pub fn lease_expires_at(&self) -> DateTime<Utc> {
        self.lease_expires_at
    }

    pub fn is_completed(&self) -> bool {
        self.status == IdempotencyStatus::Completed
    }

    pub fn result(&self) -> Option<&serde_json::Value> {
        self.result.as_ref()
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn completed_at(&self) -> Option<DateTime<Utc>> {
        self.completed_at
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub fn complete(&mut self, result: serde_json::Value) {
        self.status = IdempotencyStatus::Completed;
        self.result = Some(result);
        self.completed_at = Some(Utc::now());
    }
}
