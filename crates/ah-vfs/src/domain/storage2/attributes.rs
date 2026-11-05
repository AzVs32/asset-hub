//! Exact portable attribute values; no request/response error classification.
use bon::bon;
use std::num::NonZeroUsize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum PermissionsError {
    #[error("permission bits exceed the portable mode mask")]
    InvalidBits,
}
/// rwx and special mode bits (0o7777), independent of entry type, ownership and ACLs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Permissions(u16);
impl Permissions {
    pub fn new(bits: u32) -> Result<Self, PermissionsError> {
        if bits & !0o7777 != 0 {
            return Err(PermissionsError::InvalidBits);
        }
        Ok(Self(bits as u16))
    }
    pub fn bits(self) -> u32 {
        u32::from(self.0)
    }
}
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum XattrNameError {
    #[error("attribute name is empty")]
    Empty,
    #[error("attribute name exceeds the UTF-8 byte limit")]
    TooLong,
    #[error("attribute name contains NUL")]
    Nul,
}
/// Exact UTF-8 name, at most 255 bytes, without NUL. Backend namespace restrictions
/// are reported separately by the adapter; names are never normalized or folded.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct XattrName(String);
impl XattrName {
    pub const MAX_BYTES: usize = 255;
    pub fn parse(value: &str) -> Result<Self, XattrNameError> {
        if value.is_empty() {
            return Err(XattrNameError::Empty);
        }
        if value.len() > Self::MAX_BYTES {
            return Err(XattrNameError::TooLong);
        }
        if value.contains('\0') {
            return Err(XattrNameError::Nul);
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Existence policy enforced atomically, never with a separate lookup then write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum XattrWriteMode {
    Upsert,
    CreateNew,
    Replace,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum XattrListError {
    #[error("attribute names must be strictly increasing")]
    NotOrdered,
    #[error("attribute name list exceeds the requested byte limit")]
    TooLarge,
}
/// A complete sorted name list. The encoded size includes a NUL per name;
/// results exceeding the caller's bound fail instead of being truncated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XattrList(Vec<XattrName>);
#[bon]
impl XattrList {
    #[builder(start_fn = builder, state_mod(vis = "pub"))]
    pub fn new(names: Vec<XattrName>, max_bytes: NonZeroUsize) -> Result<Self, XattrListError> {
        if names.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(XattrListError::NotOrdered);
        }
        let mut size = 0usize;
        for name in &names {
            size = size
                .checked_add(name.as_str().len() + 1)
                .ok_or(XattrListError::TooLarge)?;
            if size > max_bytes.get() {
                return Err(XattrListError::TooLarge);
            }
        }
        Ok(Self(names))
    }
    pub fn names(&self) -> &[XattrName] {
        &self.0
    }
    pub fn into_names(self) -> Vec<XattrName> {
        self.0
    }
}
