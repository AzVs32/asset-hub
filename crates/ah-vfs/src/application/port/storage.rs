//! Storage entry operations and file byte streams, independent of plugin transport.

use super::{StoragePermissions, StorageXattrs};
use crate::application::dto::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageEntry,
    StorageMoveEntryRequest, StorageReadFileRequest, StorageRemoveEntryRequest,
    StorageStatEntryRequest, StorageWriteFileRequest,
};
use crate::domain::{StorageCapabilities, StorageError, StorageErrorKind};
use std::{future::Future, num::NonZeroUsize, pin::Pin};

/// A boxed asynchronous result that allows storage ports to be used as trait objects.
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

/// Operations on one configured storage instance, using instance-local addresses.
/// Optional operations default to UnsupportedOperation. Implementors reject
/// unsupported requests before side effects. Native backend
/// limitations never silently weaken atomicity, range or staged-write contracts.
/// Prefix adapters define persistent empty dirs and file/prefix collision policy.
pub trait Storage: Send + Sync {
    /// Effective instance support, stable for this handle. Authorization and
    /// path/device-specific restrictions are checked by the operation itself.
    /// Plugin proxies cache this declaration during open; this getter performs no I/O.
    fn capabilities(&self) -> StorageCapabilities;

    /// Whether this handle's runtime generation has failed or been replaced.
    /// This local, monotonic observation performs no I/O. Caches may reopen the
    /// instance; retained handles and streams must never be silently rebound.
    fn is_stale(&self) -> bool {
        false
    }

    /// A typed extension must be present exactly when permissions support is
    /// advertised; its read/write behavior must agree with that declaration.
    fn permissions(&self) -> Option<&dyn StoragePermissions> {
        None
    }

    /// A typed extension must agree with the advertised xattr support.
    fn xattrs(&self) -> Option<&dyn StorageXattrs> {
        None
    }

    /// Preferred transfer chunk size, shared by reads and writes. Plugin proxies
    /// cache a validated, bounded value during open; this getter performs no I/O.
    fn chunk_size(&self) -> NonZeroUsize;

    /// Retrieve entry metadata without following its final symbolic link.
    fn stat_entry(&self, _request: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        unsupported()
    }

    /// List immediate backend entries without merging VFS mount points.
    fn list_entries(&self, _query: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        unsupported()
    }

    /// Open a range-limited stream of raw file bytes, independent of this handle.
    fn read_file(
        &self,
        _request: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        unsupported()
    }

    /// Stage a complete-file write. The target changes only when commit publishes it.
    fn write_file(
        &self,
        _request: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        unsupported()
    }

    /// Create a directory in the backend.
    fn create_dir(&self, _request: StorageCreateDirRequest) -> StorageFuture<'_, StorageEntry> {
        unsupported()
    }

    /// Remove a file, link itself, or empty directory; preserve the instance root.
    fn remove_entry(&self, _request: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        unsupported()
    }

    /// Rename an entry within this instance according to its replacement policy.
    fn move_entry(&self, _request: StorageMoveEntryRequest) -> StorageFuture<'_, StorageEntry> {
        unsupported()
    }
}

/// A backend file read session retained for the lifetime of the stream.
pub trait StorageFileReader: Send {
    /// Read the next chunk; `None` denotes EOF and `Some` contains non-empty bytes.
    /// Repeated reads after EOF return `None`. After an error or cancellation,
    /// implementations may close the session rather than risk skipping bytes.
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>>;
}

/// A backend file write session whose resources remain valid until it is finished.
///
/// Commit and abort are terminal, including failed attempts; later calls return
/// a session error. Dropping an unfinished session must not publish its data;
/// cleanup is best effort. Cancellation during commit may leave publication's
/// outcome unknown; callers must inspect the target before retrying.
/// Once execution starts, cancellation of commit or abort makes the local session
/// terminal. A remote resource is tied to its original Store/session and must not be
/// transparently reopened after a crash. See the port module's plugin contract.
pub trait StorageFileWriter: Send {
    /// Accept bytes for the pending write, including a final partial chunk.
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()>;

    /// Publish the completed write and return its metadata. Publication semantics
    /// are backend-specific; this interface does not promise power-loss durability.
    /// A failure whose publication outcome cannot be determined is classified as
    /// OutcomeUnknown. Callers must not infer rollback or blindly retry it.
    fn commit(&mut self) -> StorageFuture<'_, StorageEntry>;

    /// Cancel the pending write and release its temporary resources.
    fn abort(&mut self) -> StorageFuture<'_, ()>;
}

fn unsupported<T: Send + 'static>() -> StorageFuture<'static, T> {
    Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation)) })
}

/// Verify an opened adapter before using it or persisting a mount that refers to it.
/// A false declaration is invalid adapter data, distinct from an unsupported request.
pub fn validate_storage_capabilities(
    declared: StorageCapabilities,
    storage: &dyn Storage,
) -> Result<(), StorageError> {
    let actual = storage.capabilities();
    if !declared.supports(actual)
        || actual.permissions.can_read() != storage.permissions().is_some()
        || actual.xattrs.can_read() != storage.xattrs().is_some()
    {
        return Err(StorageError::new(StorageErrorKind::InvalidData));
    }
    Ok(())
}
