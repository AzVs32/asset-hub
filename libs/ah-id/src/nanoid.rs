use crate::{Generate, InvalidIdError};
use std::{fmt, str::FromStr};

/// A validated 21-character URL-safe NanoID value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NanoId(nid::Nanoid);

#[allow(clippy::new_without_default)]
impl NanoId {
    /// Generate a fresh 21-character NanoID, rather than restoring an existing identity.
    pub fn new() -> Self {
        Self(nid::Nanoid::new())
    }
}

impl Generate for NanoId {
    fn generate() -> Self {
        Self::new()
    }
}

impl FromStr for NanoId {
    type Err = InvalidIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self).map_err(|_| InvalidIdError)
    }
}

impl TryFrom<&str> for NanoId {
    type Error = InvalidIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for NanoId {
    type Error = InvalidIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for NanoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<nid::Nanoid> for NanoId {
    fn as_ref(&self) -> &nid::Nanoid {
        &self.0
    }
}
