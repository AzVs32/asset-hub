use crate::domain::{ResourceDirectory, StorageKey};
use crate::{CoreError, ResourceError};
use futures_core::Stream;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::pin::Pin;

/// 对象存储中的扫描前缀。空字符串表示存储根命名空间。
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StoragePrefix(String);

impl StoragePrefix {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn new(value: impl Into<String>) -> Result<Self, ResourceError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Ok(Self::root());
        }
        let key = StorageKey::new(value)?;
        let value = key.as_str().trim_end_matches('/');
        if value.split('/').any(|part| part.is_empty() || part == ".") {
            return Err(ResourceError::InvalidFormat {
                field: "storage.prefix",
                reason: "prefix must contain canonical non-empty path segments",
            });
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, key: &StorageKey) -> bool {
        self.is_root()
            || key.as_str() == self.as_str()
            || key
                .as_str()
                .strip_prefix(self.as_str())
                .is_some_and(|suffix| suffix.starts_with('/'))
    }
}

impl std::fmt::Display for StoragePrefix {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for StoragePrefix {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StoragePrefix {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedBlob {
    pub key: StorageKey,
    pub size: u64,
    pub mime_type: Option<String>,
}

/// 存储扫描过程中逐项产生的条目。
///
/// 存储适配器负责把本地目录遍历、S3 分页等后端细节转换为统一的条目流，核心层
/// 不感知分页游标或具体文件系统 API。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScannedStorageEntry {
    Directory(ResourceDirectory),
    Blob(ScannedBlob),
}

/// 存储扫描流。错误作为流条目返回，以便消费方只在完整扫描成功后执行删除协调。
pub type StorageScanStream =
    Pin<Box<dyn Stream<Item = Result<ScannedStorageEntry, CoreError>> + Send + 'static>>;

#[async_trait::async_trait]
pub trait StorageScanner: Send + Sync {
    /// 流式扫描用户可见的目录和普通文件；内部 `.asset-hub` 命名空间必须排除。
    fn scan(&self, prefix: &StoragePrefix) -> StorageScanStream;

    /// 读取单个对象的当前状态；路径不存在或不是普通文件时返回 `None`。
    async fn inspect(&self, key: &StorageKey) -> Result<Option<ScannedBlob>, CoreError>;
}

#[cfg(test)]
mod tests;
