use crate::driver::DriverError;
use crate::error::VfsError;

const MAX_BYTES: usize = 64;

/// A stable, case-sensitive identifier used to select a driver implementation.
///
/// Driver kinds contain lowercase ASCII letters, digits, `.`, `-`, or `_`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DriverKind(String);

impl DriverKind {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(value: &str) -> Result<(), DriverError> {
        if value.len() > MAX_BYTES {
            return Err(DriverError::KindTooLong {
                length: value.len(),
                max: MAX_BYTES,
            });
        }

        if value.is_empty()
            || value
                .chars()
                .any(|character| !matches!(character, 'a'..='z' | '0'..='9' | '.' | '-' | '_'))
        {
            return Err(DriverError::InvalidKind);
        }

        Ok(())
    }
}

impl std::fmt::Display for DriverKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for DriverKind {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for DriverKind {
    type Error = VfsError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::validate(value)?;
        Ok(Self(value.to_owned()))
    }
}

#[cfg(test)]
mod tests;
