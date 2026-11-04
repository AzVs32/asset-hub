//! Optional typed storage attributes, independent of file formats.
use super::StorageFuture;
use crate::domain::storage::{
    EntryPermissions, StorageError, StorageErrorKind, StoragePath, XattrName, XattrWriteMode,
};
use std::num::NonZeroUsize;

/// POSIX mode bits on files/dirs. Never follow symlinks, including parents.
/// ACLs and ownership are separate. Adapters document additional access needed
/// to acquire stable handles. Read-only extensions reject writes before mutation.
pub trait StoragePermissions: Send + Sync {
    fn get_permissions(&self, path: StoragePath) -> StorageFuture<'_, EntryPermissions>;
    fn set_permissions(
        &self,
        _path: StoragePath,
        _permissions: EntryPermissions,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation)) })
    }
}
/// Opaque filesystem attributes. Never follow symlinks. Calls are live and do not
/// promise a multi-attribute transaction or a snapshot across calls.
pub trait StorageXattrs: Send + Sync {
    /// Return sorted names. max_bytes bounds encoded names including NUL separators;
    /// overflow returns TooLarge, never truncated names.
    fn list_xattrs(
        &self,
        path: StoragePath,
        max_bytes: NonZeroUsize,
    ) -> StorageFuture<'_, Vec<XattrName>>;
    /// Missing keys return AttributeNotFound; empty values are valid. Values larger
    /// than max_bytes return TooLarge rather than truncated content.
    fn get_xattr(
        &self,
        path: StoragePath,
        name: XattrName,
        max_bytes: NonZeroUsize,
    ) -> StorageFuture<'_, Vec<u8>>;
    /// CreateNew conflicts with existing keys; Replace requires an existing key.
    fn set_xattr(
        &self,
        _path: StoragePath,
        _name: XattrName,
        _value: Vec<u8>,
        _mode: XattrWriteMode,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation)) })
    }
    /// Missing keys return AttributeNotFound. Read-only extensions reject writes.
    fn remove_xattr(&self, _path: StoragePath, _name: XattrName) -> StorageFuture<'_, ()> {
        Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation)) })
    }
}
