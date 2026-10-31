use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// A stable driver implementation key supplied by a plugin or built-in provider.
///
/// Keys are non-empty and contain no whitespace or control characters.
/// Identity is case-sensitive; input is preserved without normalization.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
pub struct DriverKey(String);

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DriverKeyError {
    #[error("driver keys cannot be empty")]
    Empty,
    #[error("driver keys cannot contain whitespace or control characters")]
    InvalidCharacter,
}

impl DriverKey {
    pub fn parse(input: &str) -> Result<Self, DriverKeyError> {
        if input.is_empty() {
            return Err(DriverKeyError::Empty);
        }
        if input
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
        {
            return Err(DriverKeyError::InvalidCharacter);
        }
        Ok(Self(input.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for DriverKey {
    type Err = DriverKeyError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl TryFrom<String> for DriverKey {
    type Error = DriverKeyError;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        Self::parse(&input)
    }
}
