//! 幂等服务的运行时配置。

use serde::{Deserialize, Serialize};
use std::time::Duration;

const MIN_LEASE_DURATION_SECONDS: u64 = 1;
const MAX_LEASE_DURATION_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IdempotencyConfig {
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
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_LEASE_DURATION_SECONDS..=MAX_LEASE_DURATION_SECONDS)
            .contains(&self.lease_duration_seconds)
        {
            return Err(format!(
                "idempotency.lease_duration_seconds must be between {MIN_LEASE_DURATION_SECONDS} and {MAX_LEASE_DURATION_SECONDS} seconds; got {}",
                self.lease_duration_seconds,
            ));
        }
        Ok(())
    }

    pub fn lease_duration(&self) -> Duration {
        Duration::from_secs(self.lease_duration_seconds)
    }
}
