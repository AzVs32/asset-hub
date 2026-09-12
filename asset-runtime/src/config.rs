//! Asset Hub 核心运行时配置。

use asset_config::ConfigSection;
use asset_infra::config::{BlobConfig, DatabaseConfig};
use serde::{Deserialize, Serialize};

mod idempotency;
mod resource_edit;

pub use idempotency::IdempotencyConfig;
pub use resource_edit::ResourceEditConfig;

const DEFAULT_RESOURCE_EDIT_MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;

/// 保存在共享配置文档 `[asset]` 分区中的 Asset Hub 核心运行时配置。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AssetConfig {
    pub database: DatabaseConfig,
    pub blob: BlobConfig,
    pub resource_edit: ResourceEditConfig,
    pub idempotency: IdempotencyConfig,
}

impl ConfigSection for AssetConfig {
    const SECTION: &'static str = "asset";

    fn normalize(mut self) -> Result<Self, String> {
        self.database.validate()?;
        self.blob = self.blob.normalize()?;
        self.resource_edit.validate()?;
        self.idempotency.validate()?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
