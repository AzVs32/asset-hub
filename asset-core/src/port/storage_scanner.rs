use crate::domain::{ResourceDirectory, StorageKey};
use crate::{CoreError, ResourceError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

#[async_trait::async_trait]
pub trait StorageScanner: Send + Sync {
    /// 扫描用户可见目录；内部 `.asset-hub` 命名空间必须排除。
    async fn scan_directories(
        &self,
        prefix: &StoragePrefix,
        max_entries: usize,
    ) -> Result<Vec<ResourceDirectory>, CoreError>;

    async fn scan(
        &self,
        prefix: &StoragePrefix,
        max_entries: usize,
    ) -> Result<Vec<ScannedBlob>, CoreError>;
}

#[cfg(test)]
mod tests;
