//! A bounded storage-contract entry point on the memory driver's native state.
use super::{
    CHUNK_SIZE, EntryData, MemoryEntry, MemoryStorage, Shared, children, descendant,
    file_reader::MemoryFileReader, file_writer::MemoryFileWriter,
};
use crate::Capacity;
use ah_vfs::application::port::{
    EntrySlot, LookupScope, ObservedEntry, OperationLookup, StorageFileReader, StorageFileWriter,
    StorageFuture,
};
use ah_vfs_storage::*;
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry as MapEntry},
    sync::{Arc, MutexGuard},
    time::SystemTime,
};

type Nodes = BTreeMap<String, MemoryEntry>;

impl MemoryStorage {
    /// Provision an empty instance with optional native links at its root.
    /// Links are bootstrap metadata and each target is bounded to one chunk.
    pub fn with_root_links(
        capacity: Capacity,
        links: impl IntoIterator<Item = (EntryName, LinkTarget)>,
    ) -> Result<Self, StorageError> {
        let storage = Self::new(capacity);
        let mut nodes = storage.lock(Operation::ReadLink)?;
        for (name, target) in links {
            if target.as_bytes().len() > CHUNK_SIZE {
                return Err(rejected(Operation::ReadLink, Rejection::LimitExceeded));
            }
            let MapEntry::Vacant(slot) = nodes.entry(name.as_str().into()) else {
                return Err(rejected(Operation::ReadLink, Rejection::AlreadyExists));
            };
            slot.insert(MemoryEntry {
                data: EntryData::Link(Arc::new(target)),
                modified: SystemTime::now(),
            });
        }
        drop(nodes);
        Ok(storage)
    }

    pub(crate) fn supported_capabilities() -> Capabilities {
        Capabilities::default()
            .with_stat(true)
            .with_list(true)
            .with_read_link(true)
            .with_read(ReadSupport::Range)
            .with_remove(true)
            .with_rename(RenameSupport::Both)
            .with_mutation_binding(MutationBinding::FinalSlotAtomic)
            .with_directories(DirectorySupport::hierarchical(true, true))
            .with_write(WriteSupport::staged(WriteModes::Both, true))
    }

    fn lock(&self, operation: Operation) -> Result<MutexGuard<'_, Nodes>, StorageError> {
        lock(&self.shared, operation)
    }

    pub(super) fn resume_list_directory(
        &self,
        cursor: &DirectoryCursor,
    ) -> Result<DirectorySelection, StorageError> {
        let op = Operation::List;
        let nodes = self.lock(op)?;
        let selection = cursor.selection();
        if directory(&nodes, selection.address()).map(|id| **id) != Some(selection.identity()) {
            return Err(stale(op));
        }
        Ok(selection.clone())
    }

    pub(super) fn enumerate_directory(
        &self,
        bound: &BoundListRequest,
    ) -> Result<DirectoryPage, StorageError> {
        let op = Operation::List;
        let nodes = self.lock(op)?;
        let address = bound.selection().address();
        if directory(&nodes, address).map(|id| **id) != Some(bound.selection().identity()) {
            return Err(if bound.after().is_some() {
                stale(op)
            } else {
                rejected(op, Rejection::Conflict)
            });
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
}

fn rename(
    nodes: &mut Nodes,
    request: RenameRequest,
    same_slot: bool,
) -> Result<Entry, StorageError> {
    let op = Operation::Rename;
    let source_path = request.source();
    let destination_path = request.destination();
    let source = existing(nodes, source_path, op)?;
    let destination = if let Some(entry) = nodes.get(destination_path.as_str()) {
        let kind = entry.entry_type();
        if !same_slot
            && kind == EntryType::Directory
            && nodes
                .keys()
                .any(|key| descendant(key, destination_path.as_str()))
        {
            RenameDestination::NonEmptyDirectory
        } else {
            RenameDestination::Occupied(kind)
        }
    } else {
        RenameDestination::Missing
    };
    request
        .decide_destination(source.entry_type(), destination)
        .map_err(|reason| rejected(op, reason))?;
    let result = metadata(destination_path.clone(), source, op)?;
    if same_slot {
        return Ok(result);
    }
    // Validate every resulting address under the lock before changing the namespace.
    let mut moved = Vec::new();
    for (path, entry) in nodes.iter() {
        if path == source_path.as_str() || descendant(path, source_path.as_str()) {
            let suffix = &path[source_path.as_str().len()..];
            let destination = StoragePath::parse(&format!("{destination_path}{suffix}"))
                .map_err(|cause| super::path_error(op, cause))?;
            moved.push((path.clone(), destination.as_str().to_owned(), entry.clone()));
        }
    }
    for (old, _, _) in &moved {
        nodes.remove(old);
    }
    nodes.remove(destination_path.as_str());
    for (_, new, entry) in moved {
        nodes.insert(new, entry);
    }
    Ok(result)
}

fn rejected(op: Operation, cause: impl Into<ah_vfs::resolve::ResolveFailure>) -> StorageError {
    cause.into().without_effects(op)
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
fn metadata(path: StoragePath, entry: &MemoryEntry, op: Operation) -> Result<Entry, StorageError> {
    entry
        .metadata(path)
        .map_err(|cause| invalid(op, ContractViolation::InvalidEntry, cause))
}

pub(super) struct MemoryLookup {
    shared: Arc<Shared>,
    operation: Operation,
    missing: BTreeSet<StoragePath>,
    scope: LookupScope,
    observed: BTreeMap<StoragePath, EntryData>,
}

impl MemoryLookup {
    fn data<'a>(&self, entry: &'a ObservedEntry) -> Result<&'a EntryData, StorageError> {
        self.scope.get(entry).ok_or_else(|| stale(self.operation))
    }

    fn validate_slot(
        &self,
        op: Operation,
        slot: EntrySlot<'_>,
        path: &StoragePath,
    ) -> Result<(), StorageError> {
        self.data(slot.parent)?;
        if let Some(entry) = slot.entry {
            self.data(entry)?;
        }
        slot.validate(op, path)?;
        if self.operation != op || (slot.entry.is_none() && !self.missing.contains(path)) {
            return Err(super::error(op, Failure::InvalidBinding));
        }
        Ok(())
    }

    fn validate_entry(
        &self,
        op: Operation,
        slot: Option<EntrySlot<'_>>,
        entry: &ObservedEntry,
        path: &StoragePath,
    ) -> Result<(), StorageError> {
        let data = self.data(entry)?;
        if self.operation != op || entry.entry().path() != path || slot.is_none() != path.is_root()
        {
            return Err(super::error(op, Failure::InvalidBinding));
        }
        if let Some(slot) = slot {
            self.validate_slot(op, slot, path)?;
            if !slot
                .entry
                .map(|entry| self.data(entry))
                .transpose()?
                .is_some_and(|expected| same_entry(data, expected))
            {
                return Err(super::error(op, Failure::InvalidBinding));
            }
        }
        Ok(())
    }

    fn verify_observations(&self, nodes: &Nodes) -> Result<(), StorageError> {
        for (path, observed) in &self.observed {
            if !nodes
                .get(path.as_str())
                .is_some_and(|node| same_entry(observed, &node.data))
            {
                return Err(rejected(self.operation, Rejection::Conflict));
            }
        }
        Ok(())
    }

    fn verify_absence(
        &self,
        nodes: &Nodes,
        slot: EntrySlot<'_>,
        path: &StoragePath,
    ) -> Result<(), StorageError> {
        if slot.entry.is_none() && nodes.contains_key(path.as_str()) {
            return Err(rejected(self.operation, Rejection::Conflict));
        }
        Ok(())
    }

    pub(super) fn new(shared: Arc<Shared>, operation: Operation) -> Self {
        Self {
            shared,
            operation,
            missing: BTreeSet::new(),
            scope: LookupScope::default(),
            observed: BTreeMap::new(),
        }
    }
}

fn same_entry(left: &EntryData, right: &EntryData) -> bool {
    match (left, right) {
        (EntryData::Dir(left), EntryData::Dir(right)) => Arc::ptr_eq(left, right),
        (EntryData::File(left), EntryData::File(right)) => Arc::ptr_eq(left, right),
        (EntryData::Link(left), EntryData::Link(right)) => Arc::ptr_eq(left, right),
        _ => false,
    }
}

fn bind_observation(
    scope: &LookupScope,
    observed: &mut BTreeMap<StoragePath, EntryData>,
    address: StoragePath,
    node: &MemoryEntry,
    op: Operation,
) -> Result<ObservedEntry, StorageError> {
    let entry = metadata(address.clone(), node, op)?;
    match observed.entry(address) {
        MapEntry::Vacant(slot) => {
            slot.insert(node.data.clone());
        }
        MapEntry::Occupied(slot) if !same_entry(slot.get(), &node.data) => {
            return Err(rejected(op, Rejection::Conflict));
        }
        _ => {}
    }
    Ok(scope.bind(entry, node.data.clone()))
}

impl OperationLookup for MemoryLookup {
    fn operation(&self) -> Operation {
        self.operation
    }

    fn root(&mut self) -> StorageFuture<'_, ObservedEntry> {
        Box::pin(async move {
            let op = self.operation();
            let nodes = self.shared.lock(op)?;
            let address = StoragePath::root();
            let node = existing(&nodes, &address, op)?;
            bind_observation(&self.scope, &mut self.observed, address, node, op)
        })
    }

    fn observe<'a>(
        &'a mut self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            let op = self.operation();
            let expected = self
                .scope
                .get::<EntryData>(parent)
                .ok_or_else(|| stale(op))?;
            if !matches!(expected, EntryData::Dir(_)) {
                return Err(rejected(op, Rejection::NotDirectory));
            }
            let address = parent
                .entry()
                .path()
                .join(name)
                .map_err(|_| rejected(op, Rejection::PathTooLong))?;
            let nodes = self.shared.lock(op)?;
            if !nodes
                .get(parent.entry().path().as_str())
                .is_some_and(|node| same_entry(expected, &node.data))
            {
                return Err(rejected(op, Rejection::Conflict));
            }
            let child = match nodes.get(address.as_str()) {
                Some(child) if !self.missing.contains(&address) => child,
                Some(_) => return Err(rejected(op, Rejection::Conflict)),
                None if self.observed.contains_key(&address) => {
                    return Err(rejected(op, Rejection::Conflict));
                }
                None => {
                    self.missing.insert(address);
                    return Err(rejected(op, Rejection::NotFound));
                }
            };
            bind_observation(&self.scope, &mut self.observed, address, child, op)
        })
    }

    fn read_link<'a>(&'a self, link: &'a ObservedEntry) -> StorageFuture<'a, LinkTarget> {
        Box::pin(async move {
            let op = self.operation();
            match self.scope.get::<EntryData>(link).ok_or_else(|| stale(op))? {
                EntryData::Link(target) => Ok(target.as_ref().clone()),
                _ => Err(rejected(op, Rejection::NotSymlink)),
            }
        })
    }

    fn execute_read_link<'a>(
        &'a self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
        link: &'a ObservedEntry,
        request: ReadLinkRequest,
    ) -> StorageFuture<'a, LinkTarget> {
        Box::pin(async move {
            let op = Operation::ReadLink;
            self.validate_slot(
                op,
                EntrySlot {
                    parent,
                    name,
                    entry: Some(link),
                },
                request.path(),
            )?;
            let nodes = self.shared.lock(op)?;
            self.verify_observations(&nodes)?;
            request
                .check_target(link.entry().entry_type())
                .map_err(|reason| rejected(op, reason))?;
            match self.data(link)? {
                EntryData::Link(target) => Ok(target.as_ref().clone()),
                _ => Err(super::error(op, Failure::InvalidBinding)),
            }
        })
    }

    fn execute_stat<'a>(
        &'a self,
        slot: Option<EntrySlot<'a>>,
        entry: &'a ObservedEntry,
        request: StatRequest,
    ) -> StorageFuture<'a, Entry> {
        Box::pin(async move {
            let op = Operation::Stat;
            self.validate_entry(op, slot, entry, request.path())?;
            self.verify().await?;
            Ok(entry.entry().clone())
        })
    }

    fn execute_read<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: ReadRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            let op = Operation::Read;
            self.validate_slot(op, slot, request.path())?;
            let nodes = self.shared.lock(op)?;
            self.verify_observations(&nodes)?;
            let observed = slot
                .entry
                .ok_or_else(|| super::error(op, Failure::InvalidBinding))?;
            request
                .check_target(observed.entry().entry_type())
                .map_err(|reason| rejected(op, reason))?;
            let EntryData::File(blob) = self.data(observed)? else {
                return Err(rejected(op, Rejection::NotRegularFile));
            };
            Ok(
                Box::new(MemoryFileReader::new(blob.clone(), request.range()))
                    as Box<dyn StorageFileReader>,
            )
        })
    }

    fn execute_write<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: WriteRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            let op = Operation::Write;
            self.validate_slot(op, slot, request.path())?;
            let nodes = self.shared.lock(op)?;
            self.verify_observations(&nodes)?;
            self.verify_absence(&nodes, slot, request.path())?;
            request
                .check_target(slot.entry.map(|entry| entry.entry().entry_type()))
                .map_err(|reason| rejected(op, reason))?;
            let EntryData::Dir(parent) = self.data(slot.parent)? else {
                return Err(super::error(op, Failure::InvalidBinding));
            };
            let destination = match slot.entry.map(|entry| self.data(entry)).transpose()? {
                Some(EntryData::File(blob)) => Some(Arc::downgrade(blob)),
                None => None,
                _ => return Err(super::error(op, Failure::InvalidBinding)),
            };
            Ok(Box::new(MemoryFileWriter::new(
                self.shared.clone(),
                request.path().clone(),
                slot.parent.entry().path().clone(),
                parent.clone(),
                destination,
            )) as Box<dyn StorageFileWriter>)
        })
    }

    fn execute_create_directory<'a>(
        &'a mut self,
        slot: EntrySlot<'a>,
        request: CreateDirectoryRequest,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            let op = Operation::CreateDirectory;
            let path = request.path();
            self.validate_slot(op, slot, path)?;
            let mut nodes = self.shared.lock(op)?;
            self.verify_observations(&nodes)?;
            let admission =
                request.check_target(nodes.get(path.as_str()).map(MemoryEntry::entry_type));
            self.missing.remove(path);
            admission.map_err(|reason| rejected(op, reason))?;
            let node = MemoryEntry::dir();
            let observed =
                bind_observation(&self.scope, &mut self.observed, path.clone(), &node, op)?;
            nodes.insert(path.as_str().into(), node);
            Ok(observed)
        })
    }

    fn execute_remove<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: RemoveRequest,
    ) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let op = Operation::Remove;
            self.validate_slot(op, slot, request.path())?;
            let mut nodes = self.shared.lock(op)?;
            self.verify_observations(&nodes)?;
            let entry = slot
                .entry
                .ok_or_else(|| super::error(op, Failure::InvalidBinding))?;
            request
                .check_target(entry.entry().entry_type())
                .map_err(|reason| rejected(op, reason))?;
            if request.kind() == RemoveKind::EmptyDirectory
                && nodes
                    .keys()
                    .any(|key| descendant(key, request.path().as_str()))
            {
                return Err(rejected(op, Rejection::DirectoryNotEmpty));
            }
            nodes.remove(request.path().as_str());
            Ok(())
        })
    }

    fn execute_rename<'a>(
        &'a self,
        source: EntrySlot<'a>,
        destination: EntrySlot<'a>,
        request: RenameRequest,
    ) -> StorageFuture<'a, Entry> {
        Box::pin(async move {
            let op = Operation::Rename;
            self.validate_slot(op, source, request.source())?;
            self.validate_slot(op, destination, request.destination())?;
            if source.entry.is_none() {
                return Err(super::error(op, Failure::InvalidBinding));
            }
            let mut nodes = self.shared.lock(op)?;
            self.verify_observations(&nodes)?;
            self.verify_absence(&nodes, destination, request.destination())?;
            let same_slot = source.name == destination.name
                && same_entry(self.data(source.parent)?, self.data(destination.parent)?);
            rename(&mut nodes, request, same_slot)
        })
    }

    fn select_directory<'a>(
        &'a self,
        slot: Option<EntrySlot<'a>>,
        entry: &'a ObservedEntry,
        request: ListRequest,
    ) -> StorageFuture<'a, DirectorySelection> {
        Box::pin(async move {
            let op = Operation::List;
            self.validate_entry(op, slot, entry, request.directory())?;
            if request.after().is_some() {
                return Err(super::error(op, Failure::InvalidBinding));
            }
            self.verify().await?;
            let EntryData::Dir(id) = self.data(entry)? else {
                return Err(rejected(op, Rejection::NotDirectory));
            };
            Ok(DirectorySelection::from_lookup(
                request.directory().clone(),
                **id,
            ))
        })
    }

    fn verify(&self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let nodes = self.shared.lock(self.operation)?;
            self.verify_observations(&nodes)
        })
    }
}
