use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

const MAX_KIND_ID_LEN: usize = 256;

/// Canonical lexical value shared by target-specific kind identifiers.
///
/// A kind ID contains at least two colon-separated segments. Every segment uses lowercase ASCII
/// letters, digits, `.`, `-`, and `_`. Identity values are never trimmed or case-normalized, and
/// hierarchy remains explicit metadata rather than being inferred from the number of segments.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KindId(String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KindIdError {
    #[error("kind id cannot be blank")]
    Blank,
    #[error("kind id must not have leading or trailing whitespace")]
    NonCanonical,
    #[error("kind id must not exceed {max} characters")]
    TooLong { max: usize },
    #[error("kind id must use canonical lowercase colon-separated segments: `{value}`")]
    InvalidFormat { value: String },
}

impl KindId {
    pub fn new(value: impl Into<String>) -> Result<Self, KindIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(KindIdError::Blank);
        }
        if value.trim() != value {
            return Err(KindIdError::NonCanonical);
        }
        if value.chars().count() > MAX_KIND_ID_LEN {
            return Err(KindIdError::TooLong {
                max: MAX_KIND_ID_LEN,
            });
        }
        let valid_segment = |part: &str| {
            !part.is_empty()
                && part.chars().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || matches!(character, '.' | '-' | '_')
                })
        };
        let mut segments = value.split(':');
        let valid = segments.next().is_some_and(valid_segment)
            && segments.next().is_some_and(valid_segment)
            && segments.all(valid_segment);
        if !valid {
            return Err(KindIdError::InvalidFormat { value });
        }
        Ok(Self(value))
    }

    pub fn from_static(value: &'static str) -> Self {
        Self::new(value).expect("static kind id must be canonical")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for KindId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl AsRef<str> for KindId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<String> for KindId {
    type Error = KindIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<KindId> for String {
    fn from(value: KindId) -> Self {
        value.0
    }
}

impl FromStr for KindId {
    type Err = KindIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_canonical_namespaced_ids() {
        assert_eq!(
            KindId::new("azvs.game:directory:item_v2").unwrap().as_str(),
            "azvs.game:directory:item_v2"
        );
        for value in [
            "",
            " core:image",
            "Core:image",
            "core:image ",
            "image",
            "core::image",
            "core:image:",
        ] {
            assert!(KindId::new(value).is_err(), "`{value}` must be rejected");
        }
    }
}
