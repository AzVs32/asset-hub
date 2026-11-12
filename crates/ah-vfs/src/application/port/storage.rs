//! Storage entry operations and file byte streams, independent of plugin transport.

use super::StorageAttributes;
use ah_vfs_storage::{
    AttributeRequest, AttributeResult, AttributeSupport, BoundListRequest, Capabilities,
    CreateDirectoryRequest, DirectoryCursor, DirectoryPage, DirectorySelection, ListRequest,
    MutationBinding, ReadRequest, RemoveRequest, RenameRequest, StatRequest, WriteRequest,
};
use ah_vfs_storage::{
    BackendFailure, Entry, EntryName, EntryType, LinkTarget, ReadLinkRequest, StoragePath,
};
use ah_vfs_storage::{Failure, Operation, Rejection, StorageError};

use std::{any::Any, future::Future, num::NonZeroUsize, pin::Pin, sync::Arc};

/// A boxed asynchronous result that allows storage ports to be used as trait objects.
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

/// Operations on one configured storage instance, using instance-local addresses.
/// Optional operations default to an Unsupported rejection. Implementors reject
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

    /// The mutation binding guarantee this implementation provides. It must
    /// match the driver's declaration for every opened instance.
    fn mutation_binding(&self) -> MutationBinding {
        MutationBinding::ParentPinned
    }

    /// The declared capabilities, when available on the opened port.
    fn storage_capabilities(&self) -> Option<Capabilities> {
        None
    }

    /// Preferred transfer chunk size, shared by reads and writes. Plugin proxies
    /// cache a validated, bounded value during open; this getter performs no I/O.
    fn chunk_size(&self) -> NonZeroUsize;

    /// Open a backend scope for one operation. The caller controls traversal
    /// through exact child names and observes links without following them.
    fn begin_lookup(&self, operation: Operation) -> StorageFuture<'_, Box<dyn OperationLookup>> {
        unsupported(operation)
    }

    /// Verify a cursor's selection against the backend's retained identity.
    fn resume_directory<'a>(
        &'a self,
        _cursor: &'a DirectoryCursor,
    ) -> StorageFuture<'a, DirectorySelection> {
        unsupported(Operation::List)
    }

    /// Enumerate through the binding. Reject an expired or replaced directory.
    fn list_page<'a>(&'a self, _bound: &'a BoundListRequest) -> StorageFuture<'a, DirectoryPage> {
        unsupported(Operation::List)
    }
}

/// A scope-owned entry with canonical metadata and a backend-private value.
#[derive(Clone)]
pub struct ObservedEntry {
    scope: Arc<()>,
    entry: Entry,
    value: Arc<dyn Any + Send + Sync>,
}

impl ObservedEntry {
    pub fn entry(&self) -> &Entry {
        &self.entry
    }
}

/// Authority to issue and interpret observations for one backend operation.
#[derive(Default)]
pub struct LookupScope(Arc<()>);

impl LookupScope {
    pub fn bind(&self, entry: Entry, value: impl Any + Send + Sync) -> ObservedEntry {
        ObservedEntry {
            scope: self.0.clone(),
            entry,
            value: Arc::new(value),
        }
    }

    pub fn get<'a, T: Any>(&self, entry: &'a ObservedEntry) -> Option<&'a T> {
        if Arc::ptr_eq(&self.0, &entry.scope) {
            entry.value.downcast_ref()
        } else {
            None
        }
    }
}

/// A final name together with the parent and the observed presence or absence.
/// The backend accepts absence only for a slot it observed missing in this scope.
#[derive(Clone, Copy)]
pub struct EntrySlot<'a> {
    pub parent: &'a ObservedEntry,
    pub name: &'a EntryName,
    pub entry: Option<&'a ObservedEntry>,
}

impl EntrySlot<'_> {
    pub fn validate(self, operation: Operation, address: &StoragePath) -> Result<(), StorageError> {
        if self
            .entry
            .is_some_and(|entry| !Arc::ptr_eq(&entry.scope, &self.parent.scope))
        {
            return Err(StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::Stale),
                None,
            ));
        }
        if self.parent.entry().entry_type() != EntryType::Directory
            || self.parent.entry().path().join(self.name).as_ref() != Ok(address)
            || self
                .entry
                .is_some_and(|entry| entry.entry().path() != address)
        {
            return Err(StorageError::without_effects(
                operation,
                Failure::InvalidBinding,
                None,
            ));
        }
        Ok(())
    }
}

/// No-follow primitives and canonical execution within one backend generation.
///
/// Verify the provided observations before effects. Return Stale for a
/// foreign or expired binding and Conflict for an observed slot changed in
/// the live namespace. The driver declares whether final-slot verification
/// is atomic with the effect. A staged write retains the bindings it needs
/// to recheck the destination before publication.
pub trait OperationLookup: Send + Sync {
    fn operation(&self) -> Operation;

    /// Return the bound root directory.
    fn root(&mut self) -> StorageFuture<'_, ObservedEntry>;

    /// Bind exactly one named child without following links.
    fn observe<'a>(
        &'a mut self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
    ) -> StorageFuture<'a, ObservedEntry>;

    /// Read bytes from the observed link itself for traversal.
    fn read_link<'a>(&'a self, link: &'a ObservedEntry) -> StorageFuture<'a, LinkTarget>;

    /// Execute ReadLink against the observed final slot. Return Conflict if
    /// the namespace no longer agrees with the observations. This consumes
    /// a canonical request and reads from the same binding used in traversal.
    /// A caller-supplied address that disagrees with the slot fails with
    /// [`Failure::InvalidBinding`].
    fn execute_read_link<'a>(
        &'a self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
        link: &'a ObservedEntry,
        request: ReadLinkRequest,
    ) -> StorageFuture<'a, LinkTarget> {
        Box::pin(async move {
            let op = Operation::ReadLink;
            if !Arc::ptr_eq(&parent.scope, &link.scope) {
                return Err(StorageError::without_effects(
                    op,
                    Failure::Backend(BackendFailure::Stale),
                    None,
                ));
            }
            if self.operation() != op
                || parent.entry().entry_type() != EntryType::Directory
                || link.entry().path() != request.path()
                || parent.entry().path().join(name).as_ref() != Ok(request.path())
            {
                return Err(StorageError::without_effects(
                    op,
                    Failure::InvalidBinding,
                    None,
                ));
            }
            let target = self.read_link(link).await?;
            self.verify().await?;
            Ok(target)
        })
    }

    /// Fetch metadata from the same observation used to resolve the operand.
    fn execute_stat<'a>(
        &'a self,
        slot: Option<EntrySlot<'a>>,
        entry: &'a ObservedEntry,
        request: StatRequest,
    ) -> StorageFuture<'a, Entry> {
        Box::pin(async move {
            let op = Operation::Stat;
            if let Some(slot) = slot {
                slot.validate(op, request.path())?;
            }
            if self.operation() != op
                || request.path() != entry.entry().path()
                || slot.is_none() != request.path().is_root()
                || slot.is_some_and(|slot| {
                    !slot
                        .entry
                        .is_some_and(|expected| std::ptr::eq(expected, entry))
                })
            {
                return Err(StorageError::without_effects(
                    op,
                    Failure::InvalidBinding,
                    None,
                ));
            }
            self.verify().await?;
            Ok(entry.entry().clone())
        })
    }

    /// Open a read stream for the bound file.
    fn execute_read<'a>(
        &'a self,
        _slot: EntrySlot<'a>,
        _request: ReadRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileReader>> {
        unsupported(Operation::Read)
    }

    /// Stage a write for the bound slot. Commit rechecks the observations.
    fn execute_write<'a>(
        &'a self,
        _slot: EntrySlot<'a>,
        _request: WriteRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileWriter>> {
        unsupported(Operation::Write)
    }

    /// Create one directory at a previously observed missing name.
    fn execute_create_directory<'a>(
        &'a mut self,
        _slot: EntrySlot<'a>,
        _request: CreateDirectoryRequest,
    ) -> StorageFuture<'a, ObservedEntry> {
        unsupported(Operation::CreateDirectory)
    }

    /// Remove the entry in the bound slot.
    fn execute_remove<'a>(
        &'a self,
        _slot: EntrySlot<'a>,
        _request: RemoveRequest,
    ) -> StorageFuture<'a, ()> {
        unsupported(Operation::Remove)
    }

    /// Rename using two slots from this scope.
    fn execute_rename<'a>(
        &'a self,
        _source: EntrySlot<'a>,
        _destination: EntrySlot<'a>,
        _request: RenameRequest,
    ) -> StorageFuture<'a, Entry> {
        unsupported(Operation::Rename)
    }

    /// Retain the observed directory for listing and subsequent pages.
    fn select_directory<'a>(
        &'a self,
        _slot: Option<EntrySlot<'a>>,
        _entry: &'a ObservedEntry,
        _request: ListRequest,
    ) -> StorageFuture<'a, DirectorySelection> {
        unsupported(Operation::List)
    }

    /// Execute one action on the bound entry. The slot is absent only for
    /// the bound root. Attribute writes retain the backend's mutation guarantee.
    fn execute_attribute<'a>(
        &'a self,
        _slot: Option<(&'a ObservedEntry, &'a EntryName)>,
        _entry: &'a ObservedEntry,
        request: AttributeRequest,
    ) -> StorageFuture<'a, AttributeResult> {
        unsupported(request.operation())
    }

    /// Check observations without following links. Replaced slots in a live
    /// generation are Conflict. An invalid or retired scope is Stale.
    fn verify(&self) -> StorageFuture<'_, ()>;
}

/// One link selected by the resolver and its namespace observations. Callers cannot
/// attach this binding to a different storage handle or address.
pub trait BoundLink: Send + Sync {
    fn path(&self) -> &StoragePath;

    /// Read from the bound link and verify the observed namespace. A request
    /// with a different address fails with [`Failure::InvalidBinding`].
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

/// Check the attribute interface, access levels and mutation binding guarantee
/// against the driver declaration.
/// No instance capability negotiation or backend probing is performed.
pub fn validate_storage_extensions(
    declared: Capabilities,
    storage: &dyn Storage,
) -> Result<(), crate::application::driver_error::LifecycleError> {
    let declared_attributes = match (declared.permissions(), declared.xattrs()) {
        (AttributeSupport::None, AttributeSupport::None) => None,
        support => Some(support),
    };
    if declared_attributes != storage.attributes().map(StorageAttributes::support)
        || declared.mutation_binding() != storage.mutation_binding()
    {
        return Err(crate::application::driver_error::LifecycleError::new(
            crate::application::driver_error::LifecycleFailure::InvalidDeclaration,
        ));
    }
    Ok(())
}
