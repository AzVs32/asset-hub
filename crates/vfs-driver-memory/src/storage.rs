use crate::Capacity;
use ah_vfs::application::port::{Storage, StorageFileReader, StorageFileWriter, StorageFuture};
use ah_vfs::domain::storage::{
    StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageMoveEntryRequest,
    StorageReadFileRequest, StorageRemoveEntryRequest, StorageStatEntryRequest,
    StorageWriteFileRequest,
};
use ah_vfs_storage::WriteMode;
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use ah_vfs_storage::{Entry, EntryType, StoragePath};
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

use file_reader::MemoryFileReader;
use file_writer::MemoryFileWriter;

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
    Link(ah_vfs_storage::LinkTarget),
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

fn require_dir<'a>(
    operation: Operation,
    entries: &'a BTreeMap<String, MemoryEntry>,
    path: &str,
) -> Result<&'a Arc<ah_vfs_storage::DirectoryIdentity>, StorageError> {
    match &entries
        .get(path)
        .ok_or_else(|| error(operation, Failure::Rejected(Rejection::NotFound)))?
        .data
    {
        EntryData::Dir(identity) => Ok(identity),
        EntryData::File(_) | EntryData::Link(_) => {
            Err(error(operation, Failure::Rejected(Rejection::NotDirectory)))
        }
    }
}

fn require_parents(
    operation: Operation,
    entries: &BTreeMap<String, MemoryEntry>,
    path: &StoragePath,
) -> Result<(), StorageError> {
    let mut current = StoragePath::root();
    let mut segments = path.entry_names().peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            break;
        }
        current = current
            .join(&segment)
            .map_err(|cause| path_error(operation, cause))?;
        require_dir(operation, entries, current.as_str())?;
    }
    Ok(())
}

fn writable_target(
    operation: Operation,
    entries: &BTreeMap<String, MemoryEntry>,
    path: &StoragePath,
    mode: WriteMode,
) -> Result<(), StorageError> {
    if path.is_root() {
        return Err(error(
            operation,
            Failure::Rejected(Rejection::RootProtected),
        ));
    }
    require_parents(operation, entries, path)?;
    if let Some(entry) = entries.get(path.as_str()) {
        if mode == WriteMode::CreateNew {
            return Err(error(
                operation,
                Failure::Rejected(Rejection::AlreadyExists),
            ));
        }
        if matches!(entry.data, EntryData::Dir(_)) {
            return Err(error(operation, Failure::Rejected(Rejection::IsDirectory)));
        }
    }
    Ok(())
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

    fn stat_entry(&self, request: StorageStatEntryRequest) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let operation = Operation::Stat;
            let entries = self.shared.lock(operation)?;
            require_parents(operation, &entries, &request.path)?;
            entries
                .get(request.path.as_str())
                .ok_or_else(|| error(operation, Failure::Rejected(Rejection::NotFound)))?
                .metadata(request.path)
                .map_err(|cause| metadata_error(operation, cause))
        })
    }

    fn list_entries(&self, query: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        Box::pin(async move {
            let operation = Operation::List;
            query.validate()?;
            let entries = self.shared.lock(operation)?;
            require_parents(operation, &entries, &query.path)?;
            require_dir(operation, &entries, query.path.as_str())?;
            let mut page = Vec::new();
            let mut has_more = false;
            for (path, name, entry) in children(&entries, query.path.as_str()) {
                if query
                    .after
                    .as_ref()
                    .is_some_and(|after| name <= after.as_str())
                {
                    continue;
                }
                if page.len() == query.limit.get() {
                    has_more = true;
                    break;
                }
                page.push(
                    entry
                        .metadata(
                            StoragePath::parse(path)
                                .map_err(|cause| path_error(operation, cause))?,
                        )
                        .map_err(|cause| metadata_error(operation, cause))?,
                );
            }
            let next_after = if has_more {
                page.last().and_then(|entry| entry.path().name())
            } else {
                None
            };
            Ok(StorageDirPage {
                entries: page,
                next_after,
            })
        })
    }

    fn read_file(
        &self,
        request: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            let operation = Operation::Read;
            let entries = self.shared.lock(operation)?;
            require_parents(operation, &entries, &request.path)?;
            let entry = entries
                .get(request.path.as_str())
                .ok_or_else(|| error(operation, Failure::Rejected(Rejection::NotFound)))?;
            let EntryData::File(blob) = &entry.data else {
                return Err(error(operation, Failure::Rejected(Rejection::IsDirectory)));
            };
            Ok(Box::new(MemoryFileReader::new(
                blob.clone(),
                request.offset,
                request.length,
            )) as Box<dyn StorageFileReader>)
        })
    }

    fn write_file(
        &self,
        request: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            let operation = Operation::Write;
            let entries = self.shared.lock(operation)?;
            writable_target(operation, &entries, &request.path, request.mode)?;
            let parent = request
                .path
                .parent()
                .ok_or_else(|| error(operation, Failure::Rejected(Rejection::RootProtected)))?;
            let identity = require_dir(operation, &entries, parent.as_str())?.clone();
            Ok(Box::new(MemoryFileWriter::new(
                self.shared.clone(),
                request,
                identity,
            )) as Box<dyn StorageFileWriter>)
        })
    }

    fn create_dir(&self, request: StorageCreateDirRequest) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let operation = Operation::CreateDirectory;
            let mut entries = self.shared.lock(operation)?;
            if !request.recursive {
                require_parents(operation, &entries, &request.path)?;
            }
            if let Some(existing) = entries.get(request.path.as_str()) {
                if request.recursive {
                    require_dir(operation, &entries, request.path.as_str())?;
                    return existing
                        .metadata(request.path)
                        .map_err(|cause| metadata_error(operation, cause));
                }
                return Err(error(
                    operation,
                    Failure::Rejected(Rejection::AlreadyExists),
                ));
            }
            let mut current = StoragePath::root();
            let mut missing = Vec::new();
            for segment in request.path.entry_names() {
                current = current
                    .join(&segment)
                    .map_err(|cause| path_error(operation, cause))?;
                if entries.contains_key(current.as_str()) {
                    require_dir(operation, &entries, current.as_str())?;
                } else {
                    missing.push(current.as_str().to_owned());
                }
            }
            let target = MemoryEntry::dir();
            let result = target
                .metadata(request.path.clone())
                .map_err(|cause| metadata_error(operation, cause))?;
            for path in missing {
                let entry = if path == request.path.as_str() {
                    target.clone()
                } else {
                    MemoryEntry::dir()
                };
                entries.insert(path, entry);
            }
            Ok(result)
        })
    }

    fn remove_entry(&self, request: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let operation = Operation::Remove;
            if request.path.is_root() {
                return Err(error(
                    operation,
                    Failure::Rejected(Rejection::RootProtected),
                ));
            }
            let mut entries = self.shared.lock(operation)?;
            require_parents(operation, &entries, &request.path)?;
            let entry = entries
                .get(request.path.as_str())
                .ok_or_else(|| error(operation, Failure::Rejected(Rejection::NotFound)))?;
            if matches!(entry.data, EntryData::Dir(_))
                && entries
                    .keys()
                    .any(|path| descendant(path, request.path.as_str()))
            {
                return Err(error(
                    operation,
                    Failure::Rejected(Rejection::DirectoryNotEmpty),
                ));
            }
            entries.remove(request.path.as_str());
            Ok(())
        })
    }

    fn move_entry(&self, request: StorageMoveEntryRequest) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let operation = Operation::Rename;
            if request.source.is_root() || request.destination.is_root() {
                return Err(error(
                    operation,
                    Failure::Rejected(Rejection::RootProtected),
                ));
            }
            let mut entries = self.shared.lock(operation)?;
            require_parents(operation, &entries, &request.source)?;
            require_parents(operation, &entries, &request.destination)?;
            let source = entries
                .get(request.source.as_str())
                .ok_or_else(|| error(operation, Failure::Rejected(Rejection::NotFound)))?
                .clone();
            if request.source == request.destination {
                if !request.replace {
                    return Err(error(
                        operation,
                        Failure::Rejected(Rejection::AlreadyExists),
                    ));
                }
                return source
                    .metadata(request.destination)
                    .map_err(|cause| metadata_error(operation, cause));
            }
            if descendant(request.destination.as_str(), request.source.as_str()) {
                return Err(error(operation, Failure::Rejected(Rejection::Conflict)));
            }
            if let Some(target) = entries.get(request.destination.as_str()) {
                if !request.replace {
                    return Err(error(
                        operation,
                        Failure::Rejected(Rejection::AlreadyExists),
                    ));
                }
                match (&source.data, &target.data) {
                    (EntryData::Dir(_), EntryData::File(_) | EntryData::Link(_)) => {
                        return Err(error(operation, Failure::Rejected(Rejection::NotDirectory)));
                    }
                    (EntryData::File(_) | EntryData::Link(_), EntryData::Dir(_)) => {
                        return Err(error(operation, Failure::Rejected(Rejection::IsDirectory)));
                    }
                    (EntryData::Dir(_), EntryData::Dir(_))
                        if entries
                            .keys()
                            .any(|path| descendant(path, request.destination.as_str())) =>
                    {
                        return Err(error(
                            operation,
                            Failure::Rejected(Rejection::DirectoryNotEmpty),
                        ));
                    }
                    _ => {}
                }
            }
            // Validate every resulting address before changing the namespace.
            let mut moved = Vec::new();
            for (path, entry) in entries.iter() {
                if path == request.source.as_str() || descendant(path, request.source.as_str()) {
                    let suffix = &path[request.source.as_str().len()..];
                    let destination =
                        StoragePath::parse(&format!("{}{suffix}", request.destination))
                            .map_err(|cause| path_error(operation, cause))?;
                    moved.push((path.clone(), destination.as_str().to_owned(), entry.clone()));
                }
            }
            let result = source
                .metadata(request.destination.clone())
                .map_err(|cause| metadata_error(operation, cause))?;
            for (old, _, _) in &moved {
                entries.remove(old);
            }
            entries.remove(request.destination.as_str());
            for (_, new, entry) in moved {
                entries.insert(new, entry);
            }
            Ok(result)
        })
    }
}
