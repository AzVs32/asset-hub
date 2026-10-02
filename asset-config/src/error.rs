use std::error::Error;

/// Errors from registering, loading, or accessing configuration sections.
///
/// Registration rejects invalid or overlapping keys. Loading reports backend
/// failures or a user's validation error; accessing an absent type reports
/// [`Self::NotRegistered`]. Validation errors preserve the original boxed source
/// and its error chain through [`Error::source`].
/// Their display text identifies the section without repeating the validator's
/// reason; use `source()` or an error-chain reporter to include that reason.
///
/// This enum is non-exhaustive; callers matching variants must include a fallback.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// Default serialization, file loading, parsing, or deserialization failed.
    #[error(transparent)]
    Backend(#[from] config::ConfigError),

    /// A configuration section uses the same key as, or overlaps, another section.
    #[error("config key `{incoming}` conflicts with registered key `{existing}`")]
    KeyConflict {
        /// A previously registered section path.
        existing: &'static str,
        /// The new section path that conflicts with it.
        incoming: &'static str,
    },

    /// A section key is not a dot-separated path of bare key segments.
    #[error(
        "invalid config key `{0}`: use non-empty dot-separated segments containing only ASCII letters, digits, `_`, or `-`"
    )]
    InvalidKey(&'static str),

    /// The requested concrete type was not registered for this load.
    #[error("config `{key}` (type `{type_name}`) is not registered")]
    NotRegistered {
        /// The full Rust type name of the requested section.
        type_name: &'static str,
        /// The requested type's configuration section path.
        key: &'static str,
    },

    /// A user validation function rejected a configuration section.
    #[error("invalid config `{key}`")]
    Validation {
        /// The section path that failed validation.
        key: &'static str,
        /// The original validator error, preserving its type and error chain.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

impl ConfigError {
    /// Wraps a validator failure with its section key and original error source.
    ///
    /// Used by `#[config(validate = path)]` and available to manual
    /// [`crate::ConfigSection::validate`] implementations. Accepts string messages
    /// and typed or boxed errors convertible to a thread-safe boxed source.
    ///
    /// ```
    /// use std::error::Error;
    ///
    /// let error = asset_config::ConfigError::validate("http", "port must be positive");
    /// assert!(matches!(&error, asset_config::ConfigError::Validation { key: "http", .. }));
    /// assert_eq!(error.to_string(), "invalid config `http`");
    /// assert_eq!(error.source().unwrap().to_string(), "port must be positive");
    /// ```
    pub fn validate(key: &'static str, error: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self::Validation {
            key,
            source: error.into(),
        }
    }
}
