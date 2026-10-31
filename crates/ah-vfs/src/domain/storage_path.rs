use std::{fmt, str::FromStr};
use thiserror::Error;

/// A UTF-8 address relative to one storage instance, independent of VFS paths.
///
/// The empty string denotes the root. Other paths use `/` separators; absolute
/// paths, empty/dot segments, backslashes, colons and NUL are rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoragePath(String);

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum StoragePathError {
    #[error("storage paths must be relative")]
    Absolute,
    #[error("storage paths contain an invalid segment or character")]
    InvalidSegment,
    #[error("storage path exceeds the UTF-8 byte limit")]
    TooLong,
}

impl StoragePath {
    pub const MAX_LEN: usize = 4096;
    pub fn parse(input: &str) -> Result<Self, StoragePathError> {
        if input.len() > Self::MAX_LEN {
            return Err(StoragePathError::TooLong);
        }
        if input.starts_with('/') {
            return Err(StoragePathError::Absolute);
        }
        if input.contains(['\\', ':', '\0'])
            || (!input.is_empty() && input.split('/').any(|part| matches!(part, "" | "." | "..")))
        {
            return Err(StoragePathError::InvalidSegment);
        }
        Ok(Self(input.into()))
    }
    pub fn root() -> Self {
        Self(String::new())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
    pub fn segments(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.0.split('/').filter(|part| !part.is_empty())
    }
    pub fn name(&self) -> Option<&str> {
        self.segments().next_back()
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
    pub fn join(&self, name: &str) -> Result<Self, StoragePathError> {
        if name.is_empty() || name.contains('/') {
            return Err(StoragePathError::InvalidSegment);
        }
        Self::parse(&if self.is_root() {
            name.into()
        } else {
            format!("{}/{name}", self.0)
        })
    }
}

impl fmt::Display for StoragePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl FromStr for StoragePath {
    type Err = StoragePathError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}
impl TryFrom<String> for StoragePath {
    type Error = StoragePathError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
