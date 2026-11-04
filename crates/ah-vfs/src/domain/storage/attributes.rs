//! Validated, backend-independent storage attributes.
use super::{StorageError, StorageErrorKind};

/// POSIX rwx and special mode bits only, excluding entry type bits and ACLs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryPermissions(u32);
impl EntryPermissions {
    pub fn new(bits: u32) -> Result<Self, StorageError> {
        if bits & !0o7777 != 0 {
            return Err(StorageError::new(StorageErrorKind::InvalidRequest));
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
impl XattrName {
    pub fn parse(name: &str) -> Result<Self, StorageError> {
        if name.is_empty() || name.len() > 255 || name.contains('\0') {
            return Err(StorageError::new(StorageErrorKind::InvalidRequest));
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
