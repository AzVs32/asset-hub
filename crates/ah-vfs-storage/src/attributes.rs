//! Exact portable attribute values; no request/response error classification.
use super::{BackendFailure, ContractViolation, Operation, StoragePath};
use getset::Getters;
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
/// A nonempty UTF-8 key of at most 255 bytes, without NUL. Equality, hashing
/// and ordering use the original UTF-8 bytes. Path separators and dot components
/// have no special meaning in a key. The adapter checks native restrictions.
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
    /// Decode an authoritative backend attribute key supplied as UTF-8 bytes.
    /// Invalid UTF-8 or an invalid logical key is
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
/// Existence policy enforced atomically, never with a separate lookup then write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XattrWriteMode {
    Upsert,
    CreateNew,
    Replace,
}

/// A permissions or extended-attribute operation on one entry. Use the named
/// constructors for actions with extensible options and `..` when matching
/// those fields. The permission value and removal name are fixed payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AttributeAction {
    GetPermissions,
    SetPermissions(Permissions),
    #[non_exhaustive]
    ListXattrs {
        max_bytes: NonZeroUsize,
    },
    #[non_exhaustive]
    GetXattr {
        name: XattrName,
        max_bytes: NonZeroUsize,
    },
    #[non_exhaustive]
    SetXattr {
        name: XattrName,
        value: Vec<u8>,
        mode: XattrWriteMode,
    },
    RemoveXattr(XattrName),
}

impl AttributeAction {
    pub fn list_xattrs(max_bytes: NonZeroUsize) -> Self {
        Self::ListXattrs { max_bytes }
    }

    pub fn get_xattr(name: XattrName, max_bytes: NonZeroUsize) -> Self {
        Self::GetXattr { name, max_bytes }
    }

    pub fn set_xattr(name: XattrName, value: Vec<u8>, mode: XattrWriteMode) -> Self {
        Self::SetXattr { name, value, mode }
    }
    pub fn operation(&self) -> Operation {
        match self {
            AttributeAction::GetPermissions => Operation::GetPermissions,
            AttributeAction::SetPermissions(_) => Operation::SetPermissions,
            AttributeAction::ListXattrs { .. } => Operation::ListXattrs,
            AttributeAction::GetXattr { .. } => Operation::GetXattr,
            AttributeAction::SetXattr { .. } => Operation::SetXattr,
            AttributeAction::RemoveXattr(_) => Operation::RemoveXattr,
        }
    }

    /// The expected result alternative and the request's bound on its contents.
    pub fn expectation(&self) -> AttributeExpectation {
        match self {
            Self::GetPermissions => AttributeExpectation::Permissions,
            Self::ListXattrs { max_bytes, .. } => AttributeExpectation::Names(*max_bytes),
            Self::GetXattr { max_bytes, .. } => AttributeExpectation::Value(*max_bytes),
            Self::SetPermissions(_) | Self::SetXattr { .. } | Self::RemoveXattr(_) => {
                AttributeExpectation::Unit
            }
        }
    }
}

/// An attribute request addresses the final directory entry itself.
///
/// On a symbolic link, an action the backend cannot apply to the link itself
/// fails with [`crate::Rejection::Unsupported`] without effects. The backend
/// never applies the action to the link target.
///
/// The address is canonical. Use [`Self::into_parts`] when
/// a protocol message or queued operation needs to own the address and action.
#[derive(Clone, Debug, Eq, PartialEq, Getters)]
pub struct AttributeRequest {
    #[getset(get = "pub")]
    path: StoragePath,
    #[getset(get = "pub")]
    action: AttributeAction,
}

impl AttributeRequest {
    pub fn new(path: StoragePath, action: AttributeAction) -> Self {
        Self { path, action }
    }

    /// Move out the validated path and action, including any attribute value
    /// buffer, without cloning them.
    pub fn into_parts(self) -> (StoragePath, AttributeAction) {
        (self.path, self.action)
    }

    pub fn operation(&self) -> Operation {
        self.action.operation()
    }
}

/// A completed attribute operation. Empty binary data and an empty list are valid.
#[derive(Debug, Eq, PartialEq)]
pub enum AttributeResult {
    Permissions(Permissions),
    Names(XattrList),
    Value(Vec<u8>),
    Unit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeExpectation {
    Permissions,
    Names(NonZeroUsize),
    Value(NonZeroUsize),
    Unit,
}

impl AttributeExpectation {
    pub fn validate(self, result: &AttributeResult) -> Result<(), ContractViolation> {
        let valid = match (self, result) {
            (Self::Permissions, AttributeResult::Permissions(_))
            | (Self::Unit, AttributeResult::Unit) => true,
            (Self::Names(limit), AttributeResult::Names(names)) => {
                names.encoded_len() <= limit.get()
            }
            (Self::Value(limit), AttributeResult::Value(value)) => value.len() <= limit.get(),
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(ContractViolation::InvalidAttributes)
        }
    }
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
impl XattrList {
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
    pub fn encoded_len(&self) -> usize {
        self.0.iter().map(|name| name.as_str().len() + 1).sum()
    }
    pub fn names(&self) -> &[XattrName] {
        &self.0
    }
    pub fn into_names(self) -> Vec<XattrName> {
        self.0
    }
}
