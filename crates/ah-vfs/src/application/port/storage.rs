//! Storage entry operations and file byte streams, independent of plugin transport.

use super::StorageAttributes;
use ah_vfs_storage::{
    AttributeSupport, BoundListRequest, Capabilities, DirectoryPage, DirectorySelection,
    ListRequest,
};
use ah_vfs_storage::{
    Entry, IntermediateSymlinks, LinkTarget, PathExpr, ReadLinkRequest, ReadRequest, StatRequest,
    StoragePath, WriteRequest,
};
use ah_vfs_storage::{Failure, Operation, Rejection, StorageError};

use std::{future::Future, num::NonZeroUsize, pin::Pin};

/// A boxed asynchronous result that allows storage ports to be used as trait objects.
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

/// Selects a base directory on the receiver's instance and an intermediate link policy.
/// The adapter binds the selected directory for the duration of the call.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LookupContext {
    pub working_directory: StoragePath,
    pub intermediate_symlinks: IntermediateSymlinks,
}

/// Operations on one configured storage instance, using instance-local addresses.
/// Optional operations default to UnsupportedOperation. Implementors reject
/// unsupported requests before side effects. Native backend
/// limitations never silently weaken atomicity, range or staged-write contracts.
/// Prefix adapters define persistent empty dirs and file/prefix collision policy.
pub trait Storage: Send + Sync {
    /// Whether this handle's runtime generation has failed or been replaced.
    /// This local, monotonic observation performs no I/O. Caches may reopen the
    /// instance; retained handles and streams must never be silently rebound.
    fn is_stale(&self) -> bool {
        false
    }

    /// Present when the driver implements at least one attribute family.
    fn attributes(&self) -> Option<&dyn StorageAttributes> {
        None
    }

    /// Preferred transfer chunk size, shared by reads and writes. Plugin proxies
    /// cache a validated, bounded value during open; this getter performs no I/O.
    fn chunk_size(&self) -> NonZeroUsize;

    /// Retrieve metadata for the entry selected by the request's lookup policy.
    fn stat_entry(
        &self,
        _request: StatRequest,
        _context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        unsupported(Operation::Stat)
    }

    /// Resolve an operand while preserving its final link and retain the backend's binding.
    fn bind_link(
        &self,
        _operand: PathExpr,
        _context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn BoundLink>> {
        unsupported(Operation::ReadLink)
    }

    /// Resolve and read one link within the binding created for this operation.
    fn read_link(&self, operand: PathExpr, context: LookupContext) -> StorageFuture<'_, LinkTarget> {
        Box::pin(async move {
            let binding = self.bind_link(operand, context).await?;
            binding
                .read(ReadLinkRequest::new(binding.path().clone()))
                .await
        })
    }

    /// Resolve the initial operand or independently verify the cursor's directory.
    fn select_directory<'a>(
        &'a self,
        _request: &'a ListRequest,
        _context: LookupContext,
    ) -> StorageFuture<'a, DirectorySelection> {
        unsupported(Operation::List)
    }

    /// Enumerate through the binding. Reject an expired or replaced directory.
    fn list_page<'a>(
        &'a self,
        _bound: &'a BoundListRequest<'_>,
    ) -> StorageFuture<'a, DirectoryPage> {
        unsupported(Operation::List)
    }

    /// Select a directory separately before enumerating a page.
    fn list_entries<'a>(
        &'a self,
        request: &'a ListRequest,
        context: LookupContext,
    ) -> StorageFuture<'a, DirectoryPage> {
        Box::pin(async move {
            let selection = self.select_directory(request, context).await?;
            let bound = request.bind_selection(selection).map_err(|reason| {
                StorageError::without_effects(Operation::List, Failure::Backend(reason), None)
            })?;
            self.list_page(&bound).await
        })
    }

    /// Resolve the request once and open a range-limited stream bound to the selected file.
    /// Check capabilities before acquiring a reader. Empty ranges and offsets at or
    /// past EOF still perform lookup and return a stream at EOF for a regular file.
    fn read_file(
        &self,
        _request: ReadRequest,
        _context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        unsupported(Operation::Read)
    }

    /// Stage a complete-file write. The target changes only when commit publishes it.
    fn write_file(
        &self,
        _request: WriteRequest,
        _context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        unsupported(Operation::Write)
    }

    /// Create a directory in the backend.
    fn create_dir(
        &self,
        _request: ah_vfs_storage::CreateDirectoryRequest,
        _context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        unsupported(Operation::CreateDirectory)
    }

    /// Remove one entry of the requested kind, preserving the final link.
    fn remove_entry(
        &self,
        _request: ah_vfs_storage::RemoveRequest,
        _context: LookupContext,
    ) -> StorageFuture<'_, ()> {
        unsupported(Operation::Remove)
    }

    /// Resolve both operands in one instance and perform a single atomic rename.
    fn move_entry(
        &self,
        _request: ah_vfs_storage::RenameRequest,
        _context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        unsupported(Operation::Rename)
    }
}

/// One backend-selected link and its namespace observations. Callers cannot
/// attach this binding to a different storage handle or address.
pub trait BoundLink: Send + Sync {
    fn path(&self) -> &StoragePath;

    /// Read from the bound link and verify the observed namespace. A request
    /// with a different address is a caller contract violation.
    fn read(&self, request: ReadLinkRequest) -> StorageFuture<'_, LinkTarget>;
}

/// A backend file read session retained for the lifetime of the stream.
pub trait StorageFileReader: Send {
    /// Read the next chunk; `None` denotes EOF and `Some` contains non-empty bytes.
    /// Repeated reads after EOF return `None`. Dropping the reader releases the
    /// bound resource. Failed or cancelled in-flight reads close the reader if
    /// progress cannot be determined. Retired remote resources are never reopened.
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
    /// A staging failure rules out publication and makes the stage ineligible
    /// for commit. It may be discarded immediately or released by abort/drop.
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()>;

    /// Publish the completed write and return its metadata. Publication semantics
    /// are backend-specific; this interface does not promise power-loss durability.
    /// A failure whose publication outcome cannot be determined is reported with
    /// MutationOutcome::Unknown. Callers must not infer rollback or blindly retry it.
    fn commit(&mut self) -> StorageFuture<'_, Entry>;

    /// Cancel the pending write and release its temporary resources.
    fn abort(&mut self) -> StorageFuture<'_, ()>;
}

fn unsupported<T: Send + 'static>(operation: Operation) -> StorageFuture<'static, T> {
    Box::pin(async move {
        Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::Unsupported),
            None,
        ))
    })
}

/// Check the attribute interface and access levels against the driver declaration.
/// No instance capability negotiation or backend probing is performed.
pub fn validate_storage_extensions(
    declared: Capabilities,
    storage: &dyn Storage,
) -> Result<(), crate::application::driver_error::LifecycleError> {
    let declared = match (declared.permissions(), declared.xattrs()) {
        (AttributeSupport::None, AttributeSupport::None) => None,
        support => Some(support),
    };
    if declared != storage.attributes().map(StorageAttributes::support) {
        return Err(crate::application::driver_error::LifecycleError::new(
            crate::application::driver_error::LifecycleFailure::InvalidDeclaration,
        ));
    }
    Ok(())
}
