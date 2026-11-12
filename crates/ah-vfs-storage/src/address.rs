//! Validated logical addresses. Native filesystem confinement belongs to adapters.
use super::BackendFailure;
use std::{fmt, str::FromStr};
use thiserror::Error;

/// Maximum UTF-8 byte length of a logical address within a backend namespace.
pub const MAX_PATH_BYTES: usize = 4096;

fn validate_name(value: &str) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::Empty);
    }
    if value.len() > MAX_PATH_BYTES {
        return Err(NameError::TooLong);
    }
    if matches!(value, "." | "..") {
        return Err(NameError::DotComponent);
    }
    if value.contains('/') {
        return Err(NameError::Separator);
    }
    if value.contains(['\\', ':', '\0']) {
        return Err(NameError::ForbiddenCharacter);
    }
    Ok(())
}

fn validate_path(value: &str) -> Result<(), PathError> {
    if value.len() > MAX_PATH_BYTES {
        return Err(PathError::TooLong);
    }
    if value.starts_with('/') {
        return Err(PathError::Absolute);
    }
    if !value.is_empty() {
        for name in value.split('/') {
            validate_name(name)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum NameError {
    #[error("entry name is empty")]
    Empty,
    #[error("dot components are not entry names")]
    DotComponent,
    #[error("entry name contains a path separator")]
    Separator,
    #[error("entry name contains a forbidden character")]
    ForbiddenCharacter,
    #[error("entry name exceeds the logical address limit")]
    TooLong,
}

/// One exact UTF-8 component. Equality, hashing and ordering use the
/// original UTF-8 bytes, without normalization or case folding.
///
/// Empty names, `.`, `..`, slash, backslash, colon and NUL are excluded.
/// Backslash and colon are reserved for portability across native path
/// syntaxes, including Windows separators, drive prefixes and stream names.
/// The logical byte limit is [`MAX_PATH_BYTES`]. The adapter checks native
/// component limits separately.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryName(String);
impl EntryName {
    pub fn parse(value: &str) -> Result<Self, NameError> {
        validate_name(value)?;
        Ok(Self(value.into()))
    }
    /// Decode the authoritative bytes of a backend directory entry name.
    /// Invalid UTF-8 or an invalid logical component is
    /// [`BackendFailure::UnrepresentableName`]. For a logical input or a
    /// protocol claim, use [`Self::parse`] and classify at that boundary.
    pub fn from_backend_bytes(value: &[u8]) -> Result<Self, BackendFailure> {
        let text = std::str::from_utf8(value).map_err(|_| BackendFailure::UnrepresentableName)?;
        Self::parse(text).map_err(|_| BackendFailure::UnrepresentableName)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for EntryName {
    type Err = NameError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}
impl TryFrom<String> for EntryName {
    type Error = NameError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_name(&value)?;
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum PathError {
    #[error("storage address must be relative")]
    Absolute,
    #[error("storage address exceeds the UTF-8 byte limit")]
    TooLong,
    #[error("storage address contains an invalid component")]
    InvalidComponent(#[from] NameError),
}
/// An exact logical address within a backend namespace. The empty string
/// denotes its root. An address alone does not attest to backend existence.
/// Equality, hashing and ordering use case-sensitive UTF-8 text. No OS access,
/// Unicode normalization, dot resolution or symbolic-link traversal is performed.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StoragePath(String);
impl StoragePath {
    pub fn parse(value: &str) -> Result<Self, PathError> {
        validate_path(value)?;
        Ok(Self(value.into()))
    }
    pub fn root() -> Self {
        Self::default()
    }
    /// The address text within the backend namespace, with no leading slash.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
    pub fn name(&self) -> Option<EntryName> {
        self.split_name().map(|(_, name)| EntryName(name.into()))
    }
    pub fn parent(&self) -> Option<Self> {
        self.split_name().map(|(parent, _)| Self(parent.into()))
    }
    /// Iterate over the validated components of this address in order.
    pub fn entry_names(&self) -> impl DoubleEndedIterator<Item = EntryName> + '_ {
        // The address already contains only validated names. The root has none.
        self.0
            .split_terminator('/')
            .map(|name| EntryName(name.into()))
    }
    pub(super) fn split_name(&self) -> Option<(&str, &str)> {
        if self.is_root() {
            None
        } else {
            Some(self.0.rsplit_once('/').unwrap_or(("", self.as_str())))
        }
    }
    /// Join exactly one validated component and recheck the total byte bound.
    pub fn join(&self, name: &EntryName) -> Result<Self, PathError> {
        let extra = usize::from(!self.is_root()) + name.as_str().len();
        if self.0.len() + extra > MAX_PATH_BYTES {
            return Err(PathError::TooLong);
        }
        Ok(Self(if self.is_root() {
            name.0.clone()
        } else {
            format!("{}/{}", self.0, name.0)
        }))
    }
}

impl fmt::Display for StoragePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for StoragePath {
    type Err = PathError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}
impl TryFrom<String> for StoragePath {
    type Error = PathError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_path(&value)?;
        Ok(Self(value))
    }
}
