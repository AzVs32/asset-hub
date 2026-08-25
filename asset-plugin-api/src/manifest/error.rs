use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable classification for Manifest and package-lock validation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ManifestValidationCode {
    UnsupportedVersion,
    InvalidValue,
    InconsistentDeclaration,
    ActionOwnerMismatch,
}

/// Machine-readable validation failure with a JSON-path-like field location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManifestValidationError {
    pub code: ManifestValidationCode,
    pub path: String,
    pub message: String,
}

impl ManifestValidationError {
    pub(crate) fn new(
        code: ManifestValidationCode,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ManifestValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ManifestValidationError {}
