use crate::error::VfsError;
use crate::namespace::NamespaceError;

const MAX_BYTES: usize = 255;

/// One validated name in the virtual namespace.
///
/// Names are case-sensitive, preserve spaces and Unicode exactly, and measure
/// their length in UTF-8 bytes. Separators, dot names, backslashes, and control
/// characters are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryName(String);

impl EntryName {
    /// Returns the validated name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn validate(name: &str) -> Result<(), NamespaceError> {
        if name.is_empty()
            || matches!(name, "." | "..")
            || name
                .chars()
                .any(|character| matches!(character, '/' | '\\') || character.is_control())
        {
            return Err(NamespaceError::InvalidEntryName);
        }

        if name.len() > MAX_BYTES {
            return Err(NamespaceError::EntryNameTooLong {
                length: name.len(),
                max: MAX_BYTES,
            });
        }

        Ok(())
    }
}

impl std::fmt::Display for EntryName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for EntryName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for EntryName {
    type Error = VfsError;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::validate(name)?;
        Ok(Self(name.to_owned()))
    }
}

#[cfg(test)]
mod tests;
