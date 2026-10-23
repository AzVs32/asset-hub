use crate::{Generate, InvalidIdError};
use std::{fmt, str::FromStr};

/// A ULID value, displayed as 26 canonical Crockford Base32 characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ulid(::ulid::Ulid);

#[allow(clippy::new_without_default)]
impl Ulid {
    /// Generate a fresh ULID, rather than restoring an existing identity.
    pub fn new() -> Self {
        Self(::ulid::Ulid::generate())
    }
}

impl Generate for Ulid {
    fn generate() -> Self {
        Self::new()
    }
}

impl FromStr for Ulid {
    type Err = InvalidIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // A 26-character Base32 number must fit in 128 bits. The upstream
        // parser can truncate overflowing bits, so reject them before parsing.
        if !matches!(value.as_bytes().first(), Some(b'0'..=b'7')) {
            return Err(InvalidIdError);
        }
        value.parse().map(Self).map_err(|_| InvalidIdError)
    }
}

impl TryFrom<&str> for Ulid {
    type Error = InvalidIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for Ulid {
    type Error = InvalidIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<::ulid::Ulid> for Ulid {
    fn as_ref(&self) -> &::ulid::Ulid {
        &self.0
    }
}
