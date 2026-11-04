use crate::Capacity;
use ah_vfs::{
    application::port::{Storage, StorageFileReader, StorageFileWriter, StorageFuture},
    domain::storage::{
        StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageEntry, StorageEntryKind,
        StorageError, StorageErrorKind, StorageMoveEntryRequest, StoragePath,
        StorageReadFileRequest, StorageRemoveEntryRequest, StorageStatEntryRequest,
        StorageWriteFileMode, StorageWriteFileRequest,
    },
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

mod file_reader;
mod file_writer;

use file_reader::MemoryFileReader;
use file_writer::MemoryFileWriter;

const CHUNK_SIZE: usize = 64 * 1024;

fn error(kind: StorageErrorKind) -> StorageError {
    StorageError::new(kind)
}

struct Quota {
    limit: u64,
    used: AtomicU64,
}

impl Quota {
    fn reserve(&self, bytes: u64) -> Result<(), StorageError> {
        self.used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|total| *total <= self.limit)
            })
            .map(|_| ())
            .map_err(|_| error(StorageErrorKind::CapacityExceeded))
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
    fn append(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        self.quota.reserve(bytes.len() as u64)?;
        if let Err(source) = self.bytes.try_reserve_exact(bytes.len()) {
            self.quota.release(bytes.len() as u64);
            return Err(StorageError::with_source(
                StorageErrorKind::CapacityExceeded,
                source,
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

impl Drop for Blob {
    fn drop(&mut self) {
        self.quota.release(self.bytes.len() as u64);
    }
}

#[derive(Clone)]
enum EntryData {
    Dir(Arc<()>),
    File(Arc<Blob>),
}

#[derive(Clone)]
struct Entry {
    data: EntryData,
    modified: SystemTime,
}

impl Entry {
    fn dir() -> Self {
        Self {
            data: EntryData::Dir(Arc::new(())),
            modified: SystemTime::now(),
        }
    }

    fn metadata(&self, path: StoragePath) -> StorageEntry {
        let (kind, size) = match &self.data {
            EntryData::Dir(_) => (StorageEntryKind::Dir, None),
            EntryData::File(blob) => (StorageEntryKind::File, Some(blob.bytes.len() as u64)),
        };
        StorageEntry {
            path,
            kind,
            size,
            modified: Some(self.modified),
        }
    }
}

struct Shared {
    entries: Mutex<BTreeMap<String, Entry>>,
    quota: Arc<Quota>,
}

impl Shared {
    fn lock(&self) -> Result<MutexGuard<'_, BTreeMap<String, Entry>>, StorageError> {
        self.entries.lock().map_err(|_| {
            StorageError::with_source(
                StorageErrorKind::Backend,
                std::io::Error::other("memory namespace lock poisoned"),
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

fn require_dir<'a>(
    entries: &'a BTreeMap<String, Entry>,
    path: &str,
) -> Result<&'a Arc<()>, StorageError> {
    match &entries
        .get(path)
        .ok_or_else(|| error(StorageErrorKind::NotFound))?
        .data
    {
        EntryData::Dir(identity) => Ok(identity),
        EntryData::File(_) => Err(error(StorageErrorKind::NotDir)),
    }
}

fn require_parents(
    entries: &BTreeMap<String, Entry>,
    path: &StoragePath,
) -> Result<(), StorageError> {
    let mut current = StoragePath::root();
    let mut segments = path.segments().peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            break;
        }
        current = current.join(segment)?;
        require_dir(entries, current.as_str())?;
    }
    Ok(())
}

fn writable_target(
    entries: &BTreeMap<String, Entry>,
    path: &StoragePath,
    mode: StorageWriteFileMode,
) -> Result<(), StorageError> {
    if path.is_root() {
        return Err(error(StorageErrorKind::RootProtected));
    }
    require_parents(entries, path)?;
    if let Some(entry) = entries.get(path.as_str()) {
        if mode == StorageWriteFileMode::CreateNew {
            return Err(error(StorageErrorKind::AlreadyExists));
        }
        if matches!(entry.data, EntryData::Dir(_)) {
            return Err(error(StorageErrorKind::IsDir));
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
                entries: Mutex::new(BTreeMap::from([(String::new(), Entry::dir())])),
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

    fn stat_entry(&self, request: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            let entries = self.shared.lock()?;
            require_parents(&entries, &request.path)?;
            Ok(entries
                .get(request.path.as_str())
                .ok_or_else(|| error(StorageErrorKind::NotFound))?
                .metadata(request.path))
        })
    }

    fn list_entries(&self, query: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        Box::pin(async move {
            query.validate()?;
            let entries = self.shared.lock()?;
            require_parents(&entries, &query.path)?;
            require_dir(&entries, query.path.as_str())?;
            let mut page = Vec::new();
            let mut has_more = false;
            for (path, entry) in entries.iter() {
                if !descendant(path, query.path.as_str()) {
                    continue;
                }
                let relative = if query.path.is_root() {
                    path.as_str()
                } else {
                    &path[query.path.as_str().len() + 1..]
                };
                if relative.contains('/')
                    || query
                        .after
                        .as_ref()
                        .is_some_and(|after| relative <= after.as_str())
                {
                    continue;
                }
                if page.len() == query.limit.get() {
                    has_more = true;
                    break;
                }
                page.push(entry.metadata(StoragePath::parse(path)?));
            }
            let next_after = if has_more {
                page.last()
                    .and_then(|entry| entry.path.name().map(String::from))
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
            let entries = self.shared.lock()?;
            require_parents(&entries, &request.path)?;
            let entry = entries
                .get(request.path.as_str())
                .ok_or_else(|| error(StorageErrorKind::NotFound))?;
            let EntryData::File(blob) = &entry.data else {
                return Err(error(StorageErrorKind::IsDir));
            };
            let start = usize::try_from(request.offset)
                .unwrap_or(usize::MAX)
                .min(blob.bytes.len());
            let length = request
                .length
                .map(|length| usize::try_from(length).unwrap_or(usize::MAX))
                .unwrap_or(usize::MAX)
                .min(blob.bytes.len() - start);
            Ok(
                Box::new(MemoryFileReader::new(blob.clone(), start, start + length))
                    as Box<dyn StorageFileReader>,
            )
        })
    }

    fn write_file(
        &self,
        request: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            let entries = self.shared.lock()?;
            writable_target(&entries, &request.path, request.mode)?;
            let parent = request
                .path
                .parent()
                .ok_or_else(|| error(StorageErrorKind::RootProtected))?;
            let identity = require_dir(&entries, parent.as_str())?.clone();
            Ok(Box::new(MemoryFileWriter::new(
                self.shared.clone(),
                request,
                identity,
            )) as Box<dyn StorageFileWriter>)
        })
    }

    fn create_dir(&self, request: StorageCreateDirRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            let mut entries = self.shared.lock()?;
            if !request.recursive {
                require_parents(&entries, &request.path)?;
            }
            if let Some(existing) = entries.get(request.path.as_str()) {
                if request.recursive {
                    require_dir(&entries, request.path.as_str())?;
                    return Ok(existing.metadata(request.path));
                }
                return Err(error(StorageErrorKind::AlreadyExists));
            }
            let mut current = StoragePath::root();
            let mut missing = Vec::new();
            for segment in request.path.segments() {
                current = current.join(segment)?;
                if entries.contains_key(current.as_str()) {
                    require_dir(&entries, current.as_str())?;
                } else {
                    missing.push(current.as_str().to_owned());
                }
            }
            for path in missing {
                entries.insert(path, Entry::dir());
            }
            Ok(entries
                .get(request.path.as_str())
                .ok_or_else(|| error(StorageErrorKind::Backend))?
                .metadata(request.path))
        })
    }

    fn remove_entry(&self, request: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            if request.path.is_root() {
                return Err(error(StorageErrorKind::RootProtected));
            }
            let mut entries = self.shared.lock()?;
            require_parents(&entries, &request.path)?;
            let entry = entries
                .get(request.path.as_str())
                .ok_or_else(|| error(StorageErrorKind::NotFound))?;
            if matches!(entry.data, EntryData::Dir(_))
                && entries
                    .keys()
                    .any(|path| descendant(path, request.path.as_str()))
            {
                return Err(error(StorageErrorKind::DirNotEmpty));
            }
            entries.remove(request.path.as_str());
            Ok(())
        })
    }

    fn move_entry(&self, request: StorageMoveEntryRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            if request.source.is_root() || request.destination.is_root() {
                return Err(error(StorageErrorKind::RootProtected));
            }
            let mut entries = self.shared.lock()?;
            require_parents(&entries, &request.source)?;
            require_parents(&entries, &request.destination)?;
            let source = entries
                .get(request.source.as_str())
                .ok_or_else(|| error(StorageErrorKind::NotFound))?
                .clone();
            if request.source == request.destination {
                if !request.replace {
                    return Err(error(StorageErrorKind::AlreadyExists));
                }
                return Ok(source.metadata(request.destination));
            }
            if descendant(request.destination.as_str(), request.source.as_str()) {
                return Err(error(StorageErrorKind::InvalidRequest));
            }
            if let Some(target) = entries.get(request.destination.as_str()) {
                if !request.replace {
                    return Err(error(StorageErrorKind::AlreadyExists));
                }
                match (&source.data, &target.data) {
                    (EntryData::Dir(_), EntryData::File(_)) => {
                        return Err(error(StorageErrorKind::NotDir));
                    }
                    (EntryData::File(_), EntryData::Dir(_)) => {
                        return Err(error(StorageErrorKind::IsDir));
                    }
                    (EntryData::Dir(_), EntryData::Dir(_))
                        if entries
                            .keys()
                            .any(|path| descendant(path, request.destination.as_str())) =>
                    {
                        return Err(error(StorageErrorKind::DirNotEmpty));
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
                        StoragePath::parse(&format!("{}{suffix}", request.destination))?;
                    moved.push((path.clone(), destination.as_str().to_owned(), entry.clone()));
                }
            }
            for (old, _, _) in &moved {
                entries.remove(old);
            }
            entries.remove(request.destination.as_str());
            for (_, new, entry) in moved {
                entries.insert(new, entry);
            }
            Ok(source.metadata(request.destination))
        })
    }
}
