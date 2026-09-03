//! Thin application wrapper over [`IdempotencyRepository`].
//!
//! A guarded command computes a canonical request hash, calls [`IdempotencyService::begin`], and
//! then either executes and completes, replays a stored result, or returns a conflict. The wrapper
//! owns no business policy beyond the same-key/same-request rule.

use crate::CoreError;
use crate::domain::{IdempotencyKey, IdempotencyRecord};
use crate::port::IdempotencyRepository;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::sync::Arc;

#[derive(Clone)]
pub struct IdempotencyService {
    repository: Arc<dyn IdempotencyRepository>,
}

/// Decision returned by [`IdempotencyService::begin`].
pub enum IdempotencyOutcome {
    /// No prior result: execute the command, then complete or abandon.
    Execute,
    /// A completed, matching record exists: return its stored result without executing.
    Replay(serde_json::Value),
    /// The key is already taken by a different request, or is still in progress.
    Conflict,
}

impl IdempotencyService {
    pub fn new(repository: Arc<dyn IdempotencyRepository>) -> Self {
        Self { repository }
    }

    pub async fn begin(
        &self,
        key: &IdempotencyKey,
        request_hash: &str,
    ) -> Result<IdempotencyOutcome, CoreError> {
        match self.repository.find(key).await? {
            Some(record) => Ok(decide(&record, request_hash)),
            None => {
                let record = IdempotencyRecord::new(key.clone(), request_hash.to_string());
                match self.repository.insert(&record).await {
                    Ok(()) => Ok(IdempotencyOutcome::Execute),
                    Err(CoreError::Conflict { .. }) => {
                        // A concurrent caller inserted first; defer to the persisted record.
                        match self.repository.find(key).await? {
                            Some(record) => Ok(decide(&record, request_hash)),
                            None => Ok(IdempotencyOutcome::Conflict),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }

    pub async fn complete(
        &self,
        key: &IdempotencyKey,
        result: serde_json::Value,
    ) -> Result<(), CoreError> {
        self.repository.complete(key, result).await
    }

    pub async fn abandon(&self, key: &IdempotencyKey) -> Result<(), CoreError> {
        self.repository.remove(key).await
    }
}

fn decide(record: &IdempotencyRecord, request_hash: &str) -> IdempotencyOutcome {
    if record.is_completed() && record.request_hash() == request_hash {
        return IdempotencyOutcome::Replay(
            record
                .result()
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
    }
    IdempotencyOutcome::Conflict
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
