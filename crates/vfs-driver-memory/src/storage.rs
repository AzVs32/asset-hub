use crate::Capacity;
use ah_vfs::application::port::LookupContext;
use ah_vfs::application::port::{BoundLink, Storage, StorageFileReader, StorageFileWriter, StorageFuture};
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use ah_vfs_storage::{
    BoundListRequest, DirectoryPage, DirectorySelection, ListRequest, WriteRequest,
};
use ah_vfs_storage::{
    Entry, EntryType, PathExpr, ReadRequest, StatRequest, StoragePath,
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

pub(crate) mod contract;
mod file_reader;
mod file_writer;

const CHUNK_SIZE: usize = 64 * 1024;

fn path_error(operation: Operation, cause: ah_vfs_storage::PathError) -> StorageError {
    StorageError::without_effects(
        operation,
        Failure::Rejected(Rejection::PathTooLong),
        Some(Box::new(cause)),
    )
}

fn metadata_error(operation: Operation, cause: ah_vfs_storage::EntryError) -> StorageError {
    StorageError::without_effects(
        operation,
        Failure::Contract(ContractViolation::InvalidResponse),
        Some(Box::new(cause)),
    )
}

fn error(operation: Operation, kind: Failure) -> StorageError {
    StorageError::without_effects(operation, kind, None)
}

struct Quota {
    limit: u64,
    used: AtomicU64,
}

impl Quota {
    fn reserve(&self, bytes: u64) -> bool {
        self.used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|total| *total <= self.limit)
            })
            .is_ok()
    }

    fn release(&self, bytes: u64) {
        self.used.fetch_sub(bytes, Ordering::AcqRel);
    }
}

struct Blob {
    bytes: Vec<u8>,
    quota: Arc<Quota>,
}

impl Blob {
    fn append(&mut self, bytes: &[u8]) -> Result<(), BlobError> {
        if !self.quota.reserve(bytes.len() as u64) {
            return Err(BlobError::Quota);
        }
        if let Err(source) = self.bytes.try_reserve_exact(bytes.len()) {
            self.quota.release(bytes.len() as u64);
            return Err(BlobError::Allocation(source));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum BlobError {
    #[error("memory payload quota exceeded")]
    Quota,
    #[error("memory payload allocation failed")]
    Allocation(#[source] std::collections::TryReserveError),
}

impl Drop for Blob {
    fn drop(&mut self) {
        self.quota.release(self.bytes.len() as u64);
    }
}

#[derive(Clone)]
enum EntryData {
    Dir(Arc<ah_vfs_storage::DirectoryIdentity>),
    File(Arc<Blob>),
    Link(Arc<ah_vfs_storage::LinkTarget>),
}

#[derive(Clone)]
struct MemoryEntry {
    data: EntryData,
    modified: SystemTime,
}

impl MemoryEntry {
    fn dir() -> Self {
        Self {
            data: EntryData::Dir(Arc::new(ah_vfs_storage::DirectoryIdentity::from_bytes(
                *ah_id::UuidV7::new().as_ref().as_bytes(),
            ))),
            modified: SystemTime::now(),
        }
    }

    fn entry_type(&self) -> EntryType {
        match &self.data {
            EntryData::Dir(_) => EntryType::Directory,
            EntryData::File(blob) => EntryType::file(Some(blob.bytes.len() as u64)),
            EntryData::Link(_) => EntryType::Symlink,
        }
    }

    fn metadata(&self, path: StoragePath) -> Result<Entry, ah_vfs_storage::EntryError> {
        Entry::new(path, self.entry_type(), Some(self.modified))
    }
}

struct Shared {
    entries: Mutex<BTreeMap<String, MemoryEntry>>,
    quota: Arc<Quota>,
}

impl Shared {
    fn lock(
        &self,
        operation: Operation,
    ) -> Result<MutexGuard<'_, BTreeMap<String, MemoryEntry>>, StorageError> {
        self.entries.lock().map_err(|_| {
            StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::Other),
                Some(Box::new(std::io::Error::other(
                    "memory namespace lock poisoned",
                ))),
            )
        })
    }
}

fn descendant(path: &str, parent: &str) -> bool {
    !path.is_empty()
        && (parent.is_empty()
            || path
                .strip_prefix(parent)
                .is_some_and(|tail| tail.starts_with('/')))
}

fn children<'a>(
    entries: &'a BTreeMap<String, MemoryEntry>,
    parent: &'a str,
) -> impl Iterator<Item = (&'a str, &'a str, &'a MemoryEntry)> {
    entries.iter().filter_map(move |(path, entry)| {
        if !descendant(path, parent) {
            return None;
        }
        let name = if parent.is_empty() {
            path.as_str()
        } else {
            &path[parent.len() + 1..]
        };
        (!name.contains('/')).then_some((path.as_str(), name, entry))
    })
}

/// Hierarchical volatile storage. Namespace mutations and publication are atomic
/// under one instance lock. Readers retain immutable versions of opened files.
///
/// Capacity counts stored payload bytes, staged writes and old versions retained
/// by readers, not metadata, allocator overhead or chunks returned to callers.
/// Replacement needs room for the old and staged versions until publication.
pub struct MemoryStorage {
    capacity: Capacity,
    shared: Arc<Shared>,
}

impl MemoryStorage {
    /// Create a fresh instance with an empty, persistent-in-runtime root directory.
    pub fn new(capacity: Capacity) -> Self {
        Self {
            capacity,
            shared: Arc::new(Shared {
                entries: Mutex::new(BTreeMap::from([(String::new(), MemoryEntry::dir())])),
                quota: Arc::new(Quota {
                    limit: capacity.bytes(),
                    used: AtomicU64::new(0),
                }),
            }),
        }
    }

    pub fn capacity(&self) -> Capacity {
        self.capacity
    }

    /// Include staged bytes and deleted/replaced versions retained by readers.
    pub fn used_bytes(&self) -> u64 {
        self.shared.quota.used.load(Ordering::Acquire)
    }
}

impl Storage for MemoryStorage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(CHUNK_SIZE).unwrap_or(NonZeroUsize::MIN)
    }

    fn stat_entry(&self, request: StatRequest, context: LookupContext) -> StorageFuture<'_, Entry> {
        Box::pin(async move { self.stat_selected(&request, context) })
    }

    fn bind_link(
        &self,
        operand: PathExpr,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn BoundLink>> {
        Box::pin(async move {
            Ok(Box::new(self.bind_selected_link(operand, context)?) as Box<dyn BoundLink>)
        })
    }

    fn select_directory<'a>(
        &'a self,
        request: &'a ListRequest,
        context: LookupContext,
    ) -> StorageFuture<'a, DirectorySelection> {
        Box::pin(async move { self.select_list_directory(request, context) })
    }

    fn list_page<'a>(
        &'a self,
        bound: &'a BoundListRequest<'_>,
    ) -> StorageFuture<'a, DirectoryPage> {
        Box::pin(async move { self.enumerate_directory(bound) })
    }

    fn read_file(
        &self,
        request: ReadRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            Ok(Box::new(self.open_reader(&request, context)?) as Box<dyn StorageFileReader>)
        })
    }

    fn write_file(
        &self,
        request: WriteRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            Ok(Box::new(self.open_writer(&request, context)?) as Box<dyn StorageFileWriter>)
        })
    }

    fn create_dir(
        &self,
        request: ah_vfs_storage::CreateDirectoryRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        Box::pin(async move { self.create_directory(&request, context) })
    }

    fn remove_entry(
        &self,
        request: ah_vfs_storage::RemoveRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move { self.remove(&request, context) })
    }

    fn move_entry(
        &self,
        request: ah_vfs_storage::RenameRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        Box::pin(async move { self.rename(&request, context) })
    }
}
