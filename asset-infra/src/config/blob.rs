//! Blob 适配器配置。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_LOCAL_BLOB_ROOT: &str = "data";
const DEFAULT_LOCAL_SYNC_DEBOUNCE_MILLISECONDS: u64 = 1_000;
const DEFAULT_LOCAL_SYNC_INTERVAL_SECONDS: u64 = 30 * 60;

/// Blob 适配器配置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlobConfig {
    /// 要使用的 Blob 后端。
    pub backend: BlobBackend,
    /// 本地文件系统后端配置。
    pub local: LocalBlobConfig,
}

/// Asset Hub 支持的 Blob 后端。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlobBackend {
    /// 通过 OpenDAL 使用本地文件系统。
    #[default]
    Local,
}

/// 本地 Blob 后端配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalBlobConfig {
    /// Blob 数据域根目录；相对路径基于当前工作目录解析。
    pub root: PathBuf,
    /// 本地文件系统与 Resource 记录的同步策略。
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

/// 本地 Blob 同步任务配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalBlobSyncConfig {
    /// 是否启动文件系统监听与周期协调。
    pub enabled: bool,
    /// 文件系统事件去抖毫秒数。
    pub debounce_milliseconds: u64,
    /// 完整树周期协调的间隔秒数。
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

impl BlobConfig {
    /// 归一化 Blob 路径并校验所选后端的配置。
    pub fn normalize(mut self) -> Result<Self, String> {
        match self.backend {
            BlobBackend::Local => {
                self.local.root = normalize_path(&self.local.root)?;
                self.local.sync.validate()?;
            }
        }
        Ok(self)
    }

    pub(crate) fn local_root(&self) -> &Path {
        match self.backend {
            BlobBackend::Local => &self.local.root,
        }
    }
}

impl LocalBlobSyncConfig {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.enabled && self.debounce_milliseconds == 0 {
            return Err("blob.local.sync.debounce_milliseconds must be greater than 0".to_owned());
        }
        if self.enabled && self.reconcile_interval_seconds == 0 {
            return Err(
                "blob.local.sync.reconcile_interval_seconds must be greater than 0".to_owned(),
            );
        }
        Ok(())
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("blob.local.root must not be empty".to_owned());
    }
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|current_dir| current_dir.join(path))
        .map_err(|error| error.to_string())
}
