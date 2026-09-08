//! Canonical object-storage key values and internal namespace ownership.

use crate::error::StorageError;
use serde::{Deserialize, Deserializer, Serialize};

/// Blob storage namespace reserved for Asset Hub internals.
///
/// Resource-visible keys must not use this prefix. Storage adapters and scanners use the same
/// value to exclude staging and recovery artifacts from user-visible scans.
pub const RESERVED_BLOB_STORAGE_PREFIX: &str = ".asset-hub";

const MAX_STORAGE_KEY_LEN: usize = 1024;

/// A validated relative object-storage key.
///
/// This value is deliberately independent of Resource: Blob ports, adapters, and scanners all
/// use it without importing a business aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct StorageKey(String);

impl StorageKey {
    /// Create and validate a storage key without normalizing its spelling.
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
}
