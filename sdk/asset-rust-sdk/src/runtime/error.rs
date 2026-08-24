use std::fmt::{self, Display};

/// Error returned by Rust plugin business handlers.
///
/// This type belongs to the Rust authoring SDK and deliberately does not expose the error type of
/// any Wasm runtime. Runtime adapters convert it into their own entrypoint error after the SDK has
/// produced the stable structured Plugin API failure payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    /// Creates an SDK error from a human-readable message.
    pub fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Creates an SDK error from another displayable error without exposing its concrete type.
    pub fn from_display(error: impl Display) -> Self {
        Self::msg(error.to_string())
    }

    /// Returns the message serialized into the structured Plugin API failure.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<base64::DecodeError> for Error {
    fn from(error: base64::DecodeError) -> Self {
        Self::from_display(error)
    }
}

impl From<asset_plugin_api::abi::ContentRangeError> for Error {
    fn from(error: asset_plugin_api::abi::ContentRangeError) -> Self {
        Self::from_display(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::from_display(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::from_display(error)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::from_display(error)
    }
}

/// Result returned by Rust plugin business handlers.
pub type Result<T> = std::result::Result<T, Error>;
