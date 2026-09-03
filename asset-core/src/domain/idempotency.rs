//! Durable business idempotency for externally retried write commands.
//!
//! Idempotency is orthogonal to optimistic concurrency (CAS), compensation, and crash recovery:
//! a relocation or content-replacement intent records *how to finish or roll back* an interrupted
//! write, while an idempotency record answers *whether this exact command was already applied* and
//! replays its result. They are never collapsed into one operation journal.

use crate::ResourceError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Maximum accepted length for a client-supplied idempotency key.
pub const MAX_IDEMPOTENCY_KEY_LEN: usize = 200;

/// Client-supplied, business-scoped identity for one retryable write command.
///
/// The same key identifies the same logical command across network retries. A key is only
/// meaningful together with the command it guards; it is not a global operation identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(value: impl Into<String>) -> Result<Self, ResourceError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ResourceError::InvalidFormat {
                field: "idempotency.key",
                reason: "idempotency key must not be empty",
            });
        }
        if value.len() > MAX_IDEMPOTENCY_KEY_LEN {
            return Err(ResourceError::InvalidFormat {
                field: "idempotency.key",
                reason: "idempotency key exceeds the maximum length",
            });
        }
        if value.chars().any(char::is_control) {
            return Err(ResourceError::InvalidFormat {
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
    type Err = ResourceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdempotencyStatus {
    InProgress,
    Completed,
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
    result: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl IdempotencyRecord {
    pub fn new(key: IdempotencyKey, request_hash: String) -> Self {
        Self {
            key,
            request_hash,
            status: IdempotencyStatus::InProgress,
            result: None,
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    pub fn rehydrate(
        key: IdempotencyKey,
        request_hash: String,
        status: IdempotencyStatus,
        result: Option<serde_json::Value>,
        created_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            key,
            request_hash,
            status,
            result,
            created_at,
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

    pub fn complete(&mut self, result: serde_json::Value) {
        self.status = IdempotencyStatus::Completed;
        self.result = Some(result);
        self.completed_at = Some(Utc::now());
    }
}
