use asset_core::CoreError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Persistent request-idempotency execution lease policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IdempotencyConfig {
    /// Seconds an unrenewed execution lease remains valid after its last durable update.
    pub lease_duration_seconds: u64,
}

impl Default for IdempotencyConfig {
    fn default() -> Self {
        Self {
            lease_duration_seconds: 5 * 60,
        }
    }
}

impl IdempotencyConfig {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.lease_duration_seconds == 0 {
            return Err(CoreError::configuration(
                "idempotency.lease_duration_seconds must be greater than 0",
            ));
        }
        Ok(())
    }

    pub fn lease_duration(&self) -> Duration {
        Duration::from_secs(self.lease_duration_seconds)
    }
}
