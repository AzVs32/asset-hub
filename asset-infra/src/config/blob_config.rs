use asset_core::CoreError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{
    DEFAULT_LOCAL_BLOB_ROOT, DEFAULT_LOCAL_SYNC_DEBOUNCE_MILLISECONDS,
    DEFAULT_LOCAL_SYNC_INTERVAL_SECONDS,
};

/// 对象存储配置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlobConfig {
    /// 当前启用的 Blob 存储后端。
    pub backend: BlobBackend,
    /// 本地文件系统后端专属配置。
    pub local: LocalBlobConfig,
}

/// 可用的 Blob 存储后端。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlobBackend {
    #[default]
    Local,
}

/// 本地文件系统 Blob 后端专属配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalBlobConfig {
    /// 本地存储根目录。相对路径会在初始化时按当前工作目录转换为绝对路径。
    pub root: PathBuf,
    /// 本地文件系统与 Resource 数据库的自动同步策略。
    pub sync: LocalBlobSyncConfig,
}

impl Default for LocalBlobConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from(DEFAULT_LOCAL_BLOB_ROOT),
            sync: LocalBlobSyncConfig::default(),
        }
    }
}

/// 本地 Blob 存储自动同步配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalBlobSyncConfig {
    /// 是否启动文件系统监听和后台协调。默认启用。
    pub enabled: bool,
    /// 文件系统事件合并窗口，避免一次保存触发多次 checksum 计算。
    pub debounce_milliseconds: u64,
    /// 保底全量协调周期，用于纠正程序停机或平台事件丢失造成的偏差。
    pub reconcile_interval_seconds: u64,
}

impl Default for LocalBlobSyncConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_milliseconds: DEFAULT_LOCAL_SYNC_DEBOUNCE_MILLISECONDS,
            reconcile_interval_seconds: DEFAULT_LOCAL_SYNC_INTERVAL_SECONDS,
        }
    }
}

impl LocalBlobSyncConfig {
    pub(super) fn validate(&self) -> Result<(), CoreError> {
        if self.enabled && self.debounce_milliseconds == 0 {
            return Err(CoreError::configuration(
                "blob.local.sync.debounce_milliseconds must be greater than 0",
            ));
        }
        if self.enabled && self.reconcile_interval_seconds == 0 {
            return Err(CoreError::configuration(
                "blob.local.sync.reconcile_interval_seconds must be greater than 0",
            ));
        }
        Ok(())
    }
}
