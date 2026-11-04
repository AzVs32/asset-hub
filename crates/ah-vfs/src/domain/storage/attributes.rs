//! Validated, backend-independent storage attributes.
use thiserror::Error;

/// POSIX rwx and special mode bits only, excluding entry type bits and ACLs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryPermissions(u32);
/// Permission bits outside the portable rwx and special-mode mask.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum EntryPermissionsError {
    #[error("permissions contain bits outside the rwx and special-mode mask")]
    InvalidBits,
}
impl EntryPermissions {
    /// Validate mode bits without assigning a request or backend error category.
    pub fn new(bits: u32) -> Result<Self, EntryPermissionsError> {
        if bits & !0o7777 != 0 {
            return Err(EntryPermissionsError::InvalidBits);
        }
        Ok(Self(bits))
    }
    pub fn bits(self) -> u32 {
        self.0
    }
}
/// Portable UTF-8 attribute name: 1..=255 bytes without NUL. Backends may restrict
/// namespaces further. Names must never be converted lossily.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XattrName(String);
/// Invalid portable attribute names, independent of their input or response context.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum XattrNameError {
    #[error("attribute names cannot be empty")]
    Empty,
    #[error("attribute name exceeds the UTF-8 byte limit")]
    TooLong,
    #[error("attribute names cannot contain NUL")]
    InvalidCharacter,
}
impl XattrName {
    pub const MAX_LEN: usize = 255;

    /// Validate the exact UTF-8 name without assigning an operation error category.
    pub fn parse(name: &str) -> Result<Self, XattrNameError> {
        if name.is_empty() {
            return Err(XattrNameError::Empty);
        }
        if name.len() > Self::MAX_LEN {
            return Err(XattrNameError::TooLong);
        }
        if name.contains('\0') {
            return Err(XattrNameError::InvalidCharacter);
        }
        Ok(Self(name.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Atomic existence policy, never emulated using a separate existence check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XattrWriteMode {
    Upsert,
    CreateNew,
    Replace,
}
