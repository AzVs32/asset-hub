//! A bounded storage-contract entry point on the memory driver's native state.
use super::{
    Blob, CHUNK_SIZE, EntryData, MemoryEntry, MemoryStorage, Shared, children, descendant,
    file_reader::MemoryFileReader,
};
use crate::Capacity;
use ah_vfs_storage::*;
use std::{
    collections::{BTreeMap, btree_map::Entry as MapEntry},
    sync::{Arc, MutexGuard, Weak},
    time::SystemTime,
};

type Nodes = BTreeMap<String, MemoryEntry>;

/// One isolated instance. The working directory for this entry point is its root.
pub struct ContractStorage {
    shared: Arc<Shared>,
}

impl ContractStorage {
    /// Provision an empty instance with optional native links at its root.
    /// Links are bootstrap metadata and each target is bounded to one chunk.
    pub fn new(
        capacity: Capacity,
        links: impl IntoIterator<Item = (EntryName, LinkTarget)>,
    ) -> Result<Self, StorageError> {
        let storage = Self {
            shared: MemoryStorage::new(capacity).shared,
        };
        let mut nodes = storage.lock(Operation::ReadLink)?;
        for (name, target) in links {
            if target.as_bytes().len() > CHUNK_SIZE {
                return Err(rejected(Operation::ReadLink, Rejection::LimitExceeded));
            }
            let MapEntry::Vacant(slot) = nodes.entry(name.as_str().into()) else {
                return Err(rejected(Operation::ReadLink, Rejection::AlreadyExists));
            };
            slot.insert(MemoryEntry {
                data: EntryData::Link(target),
                modified: SystemTime::now(),
            });
        }
        drop(nodes);
        Ok(storage)
    }

    pub fn capabilities(&self) -> Capabilities {
        Capabilities::default()
            .with_stat(true)
            .with_list(true)
            .with_read_link(true)
            .with_read(ReadSupport::Range)
            .with_remove(true)
            .with_directories(DirectorySupport::hierarchical(true, true))
            .with_write(WriteSupport::staged(WriteModes::Both, true))
    }

    pub fn used_bytes(&self) -> u64 {
        self.shared
            .quota
            .used
            .load(std::sync::atomic::Ordering::Acquire)
    }

    fn lock(&self, operation: Operation) -> Result<MutexGuard<'_, Nodes>, StorageError> {
        lock(&self.shared, operation)
    }

    pub fn stat(
        &self,
        request: &StatRequest,
        links: IntermediateSymlinks,
    ) -> Result<Entry, StorageError> {
        let op = Operation::Stat;
        let mut nodes = self.lock(op)?;
        let path = select(&mut nodes, request.lookup(), links, op)?;
        metadata(path.clone(), existing(&nodes, &path, op)?, op)
    }

    pub fn read_link(
        &self,
        request: &ReadLinkRequest,
        links: IntermediateSymlinks,
    ) -> Result<LinkTarget, StorageError> {
        let op = Operation::ReadLink;
        let mut nodes = self.lock(op)?;
        let path = select(&mut nodes, request.lookup(), links, op)?;
        match &existing(&nodes, &path, op)?.data {
            EntryData::Link(target) => Ok(target.clone()),
            _ => Err(rejected(op, Rejection::NotSymlink)),
        }
    }

    pub fn read(
        &self,
        request: &ReadRequest,
        links: IntermediateSymlinks,
    ) -> Result<ContractReader, StorageError> {
        let op = Operation::Read;
        self.capabilities()
            .require_read(request)
            .map_err(|cause| rejected(op, cause))?;
        let mut nodes = self.lock(op)?;
        let path = select(&mut nodes, request.lookup(), links, op)?;
        let EntryData::File(blob) = &existing(&nodes, &path, op)?.data else {
            return Err(rejected(op, Rejection::NotRegularFile));
        };
        let range = request.range();
        Ok(ContractReader(MemoryFileReader::new(
            blob.clone(),
            range.offset(),
            range.length(),
        )))
    }

    pub fn select_directory(
        &self,
        request: &ListRequest,
        links: IntermediateSymlinks,
    ) -> Result<DirectorySelection, StorageError> {
        let op = Operation::List;
        let mut nodes = self.lock(op)?;
        let path = match request.target() {
            ListTarget::Path(_) => {
                let lookup = request
                    .lookup()
                    .ok_or_else(|| rejected(op, Rejection::Unsupported))?;
                select(&mut nodes, lookup, links, op)?
            }
            ListTarget::Continue(cursor) => cursor.selection().address().clone(),
        };
        let identity = directory(&nodes, &path).ok_or_else(|| stale(op))?;
        Ok(DirectorySelection::from_lookup(path, **identity))
    }

    pub fn list_page(&self, bound: &BoundListRequest<'_>) -> Result<DirectoryPage, StorageError> {
        let op = Operation::List;
        let nodes = self.lock(op)?;
        let address = bound.selection().address();
        if directory(&nodes, address).map(|id| **id) != Some(bound.selection().identity()) {
            return Err(stale(op));
        }
        let mut entries = Vec::new();
        let mut more = false;
        for (path, name, entry) in children(&nodes, address.as_str()) {
            if bound.after().is_some_and(|after| name <= after.as_str()) {
                continue;
            }
            if entries.len() == bound.size().get() {
                more = true;
                break;
            }
            let path = StoragePath::parse(path)
                .map_err(|cause| invalid(op, ContractViolation::InvalidEntry, cause))?;
            entries.push(metadata(path, entry, op)?);
        }
        DirectoryPage::new(bound, entries, more)
            .map_err(|cause| invalid(op, ContractViolation::InvalidPage, cause))
    }

    pub fn create_directory(
        &self,
        request: &CreateDirectoryRequest,
        links: IntermediateSymlinks,
    ) -> Result<Entry, StorageError> {
        let op = Operation::CreateDirectory;
        let mut nodes = self.lock(op)?;
        let mut walk = Walk::new(&mut nodes, request.lookup(), links);
        walk.recursive = request.mode() == CreateDirectoryMode::Recursive;
        let path = walk.start().map_err(|failure| {
            if walk.created {
                StorageError::after_dispatch(op, failure, None)
            } else {
                StorageError::without_effects(op, failure, None)
            }
        })?;
        let entry = nodes
            .entry(path.as_str().into())
            .or_insert_with(MemoryEntry::dir);
        Entry::new(path, kind(entry), Some(entry.modified)).map_err(|cause| {
            StorageError::after_dispatch(
                op,
                Failure::Contract(ContractViolation::InvalidEntry),
                Some(Box::new(cause)),
            )
        })
    }

    pub fn remove(
        &self,
        request: &RemoveRequest,
        links: IntermediateSymlinks,
    ) -> Result<(), StorageError> {
        let op = Operation::Remove;
        let mut nodes = self.lock(op)?;
        let path = select(&mut nodes, request.lookup(), links, op)?;
        if request.kind() == RemoveKind::EmptyDirectory
            && nodes.keys().any(|key| descendant(key, path.as_str()))
        {
            return Err(rejected(op, Rejection::DirectoryNotEmpty));
        }
        nodes.remove(path.as_str());
        Ok(())
    }

    pub fn write(
        &self,
        request: &WriteRequest,
        links: IntermediateSymlinks,
    ) -> Result<ContractWriter, StorageError> {
        let op = Operation::Write;
        self.capabilities()
            .require_write(request)
            .map_err(|cause| rejected(op, cause))?;
        let mut nodes = self.lock(op)?;
        let path = select(&mut nodes, request.lookup(), links, op)?;
        let parent_path = path
            .parent()
            .ok_or_else(|| rejected(op, Rejection::RootProtected))?;
        let parent = directory(&nodes, &parent_path)
            .ok_or_else(|| rejected(op, Rejection::NotDirectory))?
            .clone();
        let destination = match nodes.get(path.as_str()) {
            Some(MemoryEntry {
                data: EntryData::File(blob),
                ..
            }) => Some(Arc::downgrade(blob)),
            None => {
                request
                    .check_new_destination(&path)
                    .map_err(|cause| rejected(op, cause))?;
                None
            }
            Some(_) => return Err(rejected(op, Rejection::NotRegularFile)),
        };
        Ok(ContractWriter {
            session: Some(PendingWrite {
                shared: self.shared.clone(),
                path,
                parent_path,
                parent,
                destination,
                blob: Blob {
                    bytes: Vec::new(),
                    quota: self.shared.quota.clone(),
                },
            }),
        })
    }
}

/// An immutable file version bound by a validated range read.
pub struct ContractReader(MemoryFileReader);

impl ContractReader {
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, StorageError> {
        self.0.next_bytes().map_err(|cause| {
            StorageError::without_effects(
                Operation::Read,
                Failure::Backend(BackendFailure::CapacityExceeded),
                Some(Box::new(cause)),
            )
        })
    }
}

struct PendingWrite {
    shared: Arc<Shared>,
    path: StoragePath,
    parent_path: StoragePath,
    parent: Arc<DirectoryIdentity>,
    destination: Option<Weak<Blob>>,
    blob: Blob,
}

/// Private staged bytes, owned by one calling session. Drop discards the stage.
pub struct ContractWriter {
    session: Option<PendingWrite>,
}

impl ContractWriter {
    pub fn append(&mut self, data: &[u8]) -> Result<(), StorageError> {
        let pending = self.session.as_mut().ok_or_else(closed_writer)?;
        if let Err(cause) = pending.blob.append(data) {
            self.session.take();
            return Err(StorageError::without_effects(
                Operation::Write,
                Failure::Backend(BackendFailure::CapacityExceeded),
                Some(Box::new(cause)),
            ));
        }
        Ok(())
    }

    pub fn commit(mut self) -> Result<Entry, StorageError> {
        let op = Operation::Write;
        let pending = self.session.take().ok_or_else(closed_writer)?;
        let mut nodes = lock(&pending.shared, op)?;
        let parent_matches = directory(&nodes, &pending.parent_path)
            .is_some_and(|current| Arc::ptr_eq(current, &pending.parent));
        let destination_matches = match (&pending.destination, nodes.get(pending.path.as_str())) {
            (None, None) => true,
            (
                Some(expected),
                Some(MemoryEntry {
                    data: EntryData::File(actual),
                    ..
                }),
            ) => expected.ptr_eq(&Arc::downgrade(actual)),
            _ => false,
        };
        if !parent_matches || !destination_matches {
            return Err(rejected(op, Rejection::Conflict));
        }
        let entry = MemoryEntry {
            data: EntryData::File(Arc::new(pending.blob)),
            modified: SystemTime::now(),
        };
        let response = metadata(pending.path.clone(), &entry, op)?;
        nodes.insert(pending.path.as_str().into(), entry);
        Ok(response)
    }

    pub fn abort(self) -> Result<(), StorageError> {
        self.session.map(|_| ()).ok_or_else(closed_writer)
    }
}

fn closed_writer() -> StorageError {
    StorageError::without_effects(
        Operation::Write,
        Failure::Backend(BackendFailure::Closed),
        None,
    )
}

fn rejected(op: Operation, cause: Rejection) -> StorageError {
    StorageError::without_effects(op, Failure::Rejected(cause), None)
}
fn stale(op: Operation) -> StorageError {
    StorageError::without_effects(op, Failure::Backend(BackendFailure::Stale), None)
}
fn invalid(
    op: Operation,
    reason: ContractViolation,
    cause: impl std::error::Error + Send + Sync + 'static,
) -> StorageError {
    StorageError::without_effects(op, Failure::Contract(reason), Some(Box::new(cause)))
}
fn lock(shared: &Shared, op: Operation) -> Result<MutexGuard<'_, Nodes>, StorageError> {
    shared.entries.lock().map_err(|_| {
        StorageError::without_effects(
            op,
            Failure::Backend(BackendFailure::Other),
            Some(Box::new(std::io::Error::other(
                "memory namespace lock poisoned",
            ))),
        )
    })
}
fn existing<'a>(
    nodes: &'a Nodes,
    path: &StoragePath,
    op: Operation,
) -> Result<&'a MemoryEntry, StorageError> {
    nodes
        .get(path.as_str())
        .ok_or_else(|| rejected(op, Rejection::NotFound))
}
fn directory<'a>(nodes: &'a Nodes, path: &StoragePath) -> Option<&'a Arc<DirectoryIdentity>> {
    match &nodes.get(path.as_str())?.data {
        EntryData::Dir(id) => Some(id),
        _ => None,
    }
}
fn kind(entry: &MemoryEntry) -> EntryType {
    match &entry.data {
        EntryData::Dir(_) => EntryType::Directory,
        EntryData::File(blob) => EntryType::file(Some(blob.bytes.len() as u64)),
        EntryData::Link(_) => EntryType::Symlink,
    }
}
fn metadata(path: StoragePath, entry: &MemoryEntry, op: Operation) -> Result<Entry, StorageError> {
    Entry::new(path, kind(entry), Some(entry.modified))
        .map_err(|cause| invalid(op, ContractViolation::InvalidEntry, cause))
}

fn select(
    nodes: &mut Nodes,
    lookup: PathLookup<'_>,
    links: IntermediateSymlinks,
    op: Operation,
) -> Result<StoragePath, StorageError> {
    Walk::new(nodes, lookup, links)
        .start()
        .map_err(|failure| StorageError::without_effects(op, failure, None))
}

struct Walk<'a, 'r> {
    nodes: &'a mut Nodes,
    lookup: PathLookup<'r>,
    links: SymlinkTraversal,
    recursive: bool,
    created: bool,
}

impl<'a, 'r> Walk<'a, 'r> {
    fn new(nodes: &'a mut Nodes, lookup: PathLookup<'r>, links: IntermediateSymlinks) -> Self {
        Self {
            nodes,
            lookup,
            links: SymlinkTraversal::new(links),
            recursive: false,
            created: false,
        }
    }

    fn start(&mut self) -> Result<StoragePath, Failure> {
        let path = self.segment(self.lookup.path(), &StoragePath::root(), &[], false)?;
        if let Some(entry) = self.nodes.get(path.as_str()) {
            self.lookup
                .check_existing_target(&path, kind(entry))
                .map_err(Failure::Rejected)?;
        }
        Ok(path)
    }

    fn segment(
        &mut self,
        expr: &PathExpr,
        base: &StoragePath,
        suffix: &[PathComponent],
        in_link: bool,
    ) -> Result<StoragePath, Failure> {
        let mut path = match expr.start() {
            LookupStart::InstanceRoot => StoragePath::root(),
            LookupStart::BaseDirectory => base.clone(),
        };
        for (index, component) in expr.components().iter().enumerate() {
            if directory(self.nodes, &path).is_none() {
                return Err(Failure::Rejected(Rejection::NotDirectory));
            }
            match component {
                PathComponent::CurrentDirectory => {}
                PathComponent::ParentDirectory => {
                    path = path.parent().unwrap_or_else(StoragePath::root)
                }
                PathComponent::Name(name) => {
                    let child = path
                        .join(name)
                        .map_err(|_| Failure::Rejected(Rejection::PathTooLong))?;
                    let remaining = &expr.components()[index + 1..];
                    match self.nodes.get(child.as_str()).map(|entry| &entry.data) {
                        Some(EntryData::Link(target)) => {
                            let pending: Vec<_> = remaining.iter().chain(suffix).cloned().collect();
                            match self
                                .links
                                .encounter(self.lookup, &pending)
                                .map_err(Failure::Rejected)?
                            {
                                SymlinkAction::FollowWithinInstance => {
                                    let target = target.prepare_lookup(&path, &pending)?;
                                    path = self.segment(&target, &path, &pending, true)?;
                                }
                                SymlinkAction::Preserve => path = child,
                            }
                        }
                        Some(_) => path = child,
                        None => {
                            let missing = if in_link {
                                MissingName::LinkTarget
                            } else if remaining.is_empty() {
                                MissingName::OperandFinal
                            } else {
                                MissingName::OperandIntermediate
                            };
                            self.lookup
                                .check_missing_name(&child, missing)
                                .map_err(Failure::Rejected)?;
                            if missing == MissingName::OperandIntermediate && self.recursive {
                                self.nodes.insert(child.as_str().into(), MemoryEntry::dir());
                                self.created = true;
                            }
                            path = child;
                        }
                    }
                }
            }
        }
        if let Some(entry) = self.nodes.get(path.as_str()) {
            expr.check_directory_requirement(kind(entry) == EntryType::Directory)
                .map_err(Failure::Rejected)?;
        }
        Ok(path)
    }
}
