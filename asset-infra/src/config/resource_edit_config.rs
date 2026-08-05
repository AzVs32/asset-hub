use serde::{Deserialize, Serialize};

use super::DEFAULT_RESOURCE_EDIT_MAX_TEXT_BYTES;

/// Host-owned interactive text editing limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceEditConfig {
    pub max_text_bytes: u64,
}

impl Default for ResourceEditConfig {
    fn default() -> Self {
        Self {
            max_text_bytes: DEFAULT_RESOURCE_EDIT_MAX_TEXT_BYTES,
        }
    }
}

impl ResourceEditConfig {
    pub(super) fn validate(&self) -> Result<(), asset_core::CoreError> {
        asset_core::domain::ResourceContentEditPolicy::new(self.max_text_bytes)
            .map(|_| ())
            .map_err(|error| asset_core::CoreError::configuration(error.to_string()))
    }
}
