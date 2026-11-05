//! Validated logical addresses. Native filesystem confinement belongs to adapters.
use std::{fmt, str::FromStr};
use thiserror::Error;

/// Maximum UTF-8 byte length of an instance-relative logical address.
pub const MAX_PATH_BYTES: usize = 4096;

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

/// One exact UTF-8 component, without normalization or case folding.
/// Native component-length limits are checked by the adapter.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryName(String);
impl EntryName {
    pub fn parse(value: &str) -> Result<Self, NameError> {
        validate_name(value)?;
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
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
/// An exact, instance-relative logical address. The empty string is the root.
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
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
    pub fn name(&self) -> Option<EntryName> {
        self.0
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .map(|name| EntryName(name.into()))
    }
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            None
        } else {
            Some(Self(
                self.0
                    .rsplit_once('/')
                    .map_or("", |(parent, _)| parent)
                    .into(),
            ))
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

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("this operation requires a non-root address")]
pub struct RootPathError;

/// An address that may be renamed, removed or replaced without targeting the root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NonRootPath(StoragePath);
impl NonRootPath {
    pub fn as_path(&self) -> &StoragePath {
        &self.0
    }
    pub fn into_path(self) -> StoragePath {
        self.0
    }
}
impl TryFrom<StoragePath> for NonRootPath {
    type Error = RootPathError;
    fn try_from(value: StoragePath) -> Result<Self, Self::Error> {
        if value.is_root() {
            Err(RootPathError)
        } else {
            Ok(Self(value))
        }
    }
}
