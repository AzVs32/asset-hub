use crate::{Generate, InvalidIdError};
use std::{fmt, str::FromStr};

/// A validated UUID v7 value. There is no unchecked public constructor or default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UuidV7(uuid::Uuid);

#[allow(clippy::new_without_default)]
impl UuidV7 {
    /// Generate a fresh UUID v7, rather than restoring an existing identity.
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

impl Generate for UuidV7 {
    fn generate() -> Self {
        Self::new()
    }
}

impl TryFrom<uuid::Uuid> for UuidV7 {
    type Error = InvalidIdError;

    fn try_from(id: uuid::Uuid) -> Result<Self, Self::Error> {
        if id.get_version_num() != 7 || id.get_variant() != uuid::Variant::RFC4122 {
            return Err(InvalidIdError);
        }
        Ok(Self(id))
    }
}

impl FromStr for UuidV7 {
    type Err = InvalidIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let id: uuid::Uuid = value.parse().map_err(|_| InvalidIdError)?;
        Self::try_from(id)
    }
}

impl TryFrom<&str> for UuidV7 {
    type Error = InvalidIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for UuidV7 {
    type Error = InvalidIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for UuidV7 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<uuid::Uuid> for UuidV7 {
    fn as_ref(&self) -> &uuid::Uuid {
        &self.0
    }
}
