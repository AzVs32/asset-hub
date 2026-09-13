use crate::error::StorageError;
use serde::{Deserialize, Deserializer, Serialize};

const INTERNAL_BLOB_STORAGE_PREFIX: &str = ".asset-hub";

const MAX_STORAGE_KEY_LEN: usize = 1024;

pub(crate) fn is_internal(path: &str) -> bool {
    path == INTERNAL_BLOB_STORAGE_PREFIX
        || path
            .strip_prefix(INTERNAL_BLOB_STORAGE_PREFIX)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// 一个经过验证的相对对象存储键。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct StorageKey(String);

impl StorageKey {
    /// 此构造函数接受用户可见键和内部键；
    /// 在派生用户可见的资源键时，请使用[`Self::visible`]；
    /// 在创建内部键时，请使用[`Self::internal`]。
    pub fn new(value: impl Into<String>) -> Result<Self, StorageError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(StorageError::Blank {
                field: "storage.key",
            });
        }
        if value.chars().count() > MAX_STORAGE_KEY_LEN {
            return Err(StorageError::TooLong {
                field: "storage.key",
                max: MAX_STORAGE_KEY_LEN,
            });
        }
        if value.chars().any(char::is_control) {
            return Err(StorageError::InvalidFormat {
                field: "storage.key",
                reason: "control characters are not allowed",
            });
        }
        if value.starts_with('/') {
            return Err(StorageError::InvalidFormat {
                field: "storage.key",
                reason: "absolute paths are not allowed",
            });
        }
        if value.split('/').any(|part| part == "..") {
            return Err(StorageError::InvalidFormat {
                field: "storage.key",
                reason: "parent path segments are not allowed",
            });
        }
        Ok(Self(value))
    }

    /// Return the original validated key spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Create a key for user-visible Resource content.
    pub fn visible(value: impl Into<String>) -> Result<Self, StorageError> {
        let key = Self::new(value)?;
        if key.is_internal() {
            return Err(StorageError::InvalidFormat {
                field: "storage.key",
                reason: "internal storage keys are not user-visible",
            });
        }
        Ok(key)
    }

    /// Create a key below the internal Blob namespace.
    pub fn internal(suffix: impl AsRef<str>) -> Result<Self, StorageError> {
        let suffix = Self::new(suffix.as_ref())?;
        if suffix.is_internal() {
            return Err(StorageError::InvalidFormat {
                field: "storage.key",
                reason: "an internal key suffix must not include the internal namespace",
            });
        }
        Self::new(format!(
            "{INTERNAL_BLOB_STORAGE_PREFIX}/{}",
            suffix.as_str()
        ))
    }

    /// Return the root key of the internal Blob namespace.
    pub fn internal_root() -> Self {
        Self(INTERNAL_BLOB_STORAGE_PREFIX.to_owned())
    }

    /// 是否属于内部命名空间。
    pub fn is_internal(&self) -> bool {
        is_internal(self.as_str())
    }

    /// Whether this key has the shape accepted by the staging-storage port.
    pub fn is_upload_staging(&self) -> bool {
        self.as_str()
            .strip_prefix(INTERNAL_BLOB_STORAGE_PREFIX)
            .and_then(|suffix| suffix.strip_prefix("/uploads/"))
            .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('/'))
    }
}

impl std::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for StorageKey {
    type Err = StorageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for StorageKey {
    type Error = StorageError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for StorageKey {
    type Error = StorageError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for StorageKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_paths_and_preserves_serialized_key_spelling() {
        assert!(StorageKey::new("assets/image.png").is_ok());
        assert_eq!(
            StorageKey::new(" library / design 01.md ")
                .unwrap()
                .as_str(),
            " library / design 01.md "
        );
        assert_eq!(
            StorageKey::new("/absolute/path"),
            Err(StorageError::InvalidFormat {
                field: "storage.key",
                reason: "absolute paths are not allowed",
            })
        );
        assert_eq!(
            StorageKey::new("assets/../secret"),
            Err(StorageError::InvalidFormat {
                field: "storage.key",
                reason: "parent path segments are not allowed",
            })
        );
        assert_eq!(
            serde_json::from_str::<StorageKey>(r#""assets/image.png""#)
                .unwrap()
                .as_str(),
            "assets/image.png"
        );
        assert_eq!(
            serde_json::to_string(&StorageKey::new("assets/image.png").unwrap()).unwrap(),
            r#""assets/image.png""#
        );
        assert!(serde_json::from_str::<StorageKey>(r#""/absolute/path""#).is_err());
    }

    #[test]
    fn separates_user_visible_and_internal_keys() {
        let internal = StorageKey::internal("uploads/session").unwrap();

        assert_eq!(internal.as_str(), ".asset-hub/uploads/session");
        assert!(internal.is_internal());
        assert!(internal.is_upload_staging());
        assert!(StorageKey::internal("/uploads/session").is_err());
        assert!(
            !StorageKey::internal("content-backups/session")
                .unwrap()
                .is_upload_staging()
        );
        assert!(StorageKey::visible(".asset-hub/uploads/session").is_err());
        assert!(StorageKey::internal(".asset-hub/uploads/session").is_err());
    }
}
