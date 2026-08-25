use super::{ManifestValidationCode, ManifestValidationError};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Canonical UTF-8 package path using `/` separators on every platform.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct PluginPackagePath(String);

impl PluginPackagePath {
    pub fn new(value: impl Into<String>) -> Result<Self, ManifestValidationError> {
        let value = value.into();
        let first = value.split('/').next().unwrap_or_default().as_bytes();
        let has_windows_drive_prefix =
            first.len() >= 2 && first[0].is_ascii_alphabetic() && first[1] == b':';
        let valid = !value.is_empty()
            && !value.starts_with('/')
            && !value.ends_with('/')
            && !value.contains('\\')
            && !has_windows_drive_prefix
            && value.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && !segment.chars().any(char::is_control)
            });
        if !valid {
            return Err(ManifestValidationError::new(
                ManifestValidationCode::InvalidValue,
                "$.integrity",
                "package path must be a canonical relative UTF-8 path using `/` separators",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PluginPackagePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for PluginPackagePath {
    type Error = ManifestValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for PluginPackagePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
