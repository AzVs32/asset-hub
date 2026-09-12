//! Resource 文本编辑策略的运行时配置。

use serde::{Deserialize, Serialize};

use super::DEFAULT_RESOURCE_EDIT_MAX_TEXT_BYTES;

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
    pub(super) fn validate(&self) -> Result<(), String> {
        asset_core::resource::domain::ResourceContentEditPolicy::new(self.max_text_bytes)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}
