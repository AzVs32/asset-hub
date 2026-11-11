//! A bounded storage-contract entry point on the memory driver's native state.
use super::{
    CHUNK_SIZE, EntryData, MemoryEntry, MemoryStorage, Shared, children, descendant,
    file_reader::MemoryFileReader, file_writer::MemoryFileWriter,
};
use crate::Capacity;
use ah_vfs::application::port::{BoundLink, LookupContext, StorageFuture};
use ah_vfs::resolve::{LookupPort, Resolution, Resolver};
use ah_vfs_storage::*;
use std::{
    collections::{BTreeMap, btree_map::Entry as MapEntry},
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
            .with_directories(DirectorySupport::hierarchical(true, true))
            .with_write(WriteSupport::staged(WriteModes::Both, true))
    }

    fn lock(&self, operation: Operation) -> Result<MutexGuard<'_, Nodes>, StorageError> {
        lock(&self.shared, operation)
    }

    pub(super) fn stat_selected(
        &self,
        request: &StatRequest,
        context: LookupContext,
    ) -> Result<Entry, StorageError> {
        let op = Operation::Stat;
        let mut nodes = self.lock(op)?;
        let path = select_with_context(&mut nodes, request.lookup(), &context, op)?;
        metadata(path.clone(), existing(&nodes, &path, op)?, op)
    }

    pub(super) fn bind_selected_link(
        &self,
        operand: PathExpr,
        context: LookupContext,
    ) -> Result<MemoryLink, StorageError> {
        let op = Operation::ReadLink;
        let mut nodes = self.lock(op)?;
        let stat = StatRequest::new(operand, SymlinkAction::Preserve);
        let mut namespace = LookupNamespace::new(&mut nodes, op);
        let mut resolver = Resolver::new(op, context.intermediate_symlinks);
        let path = namespace.select(&mut resolver, stat.lookup(), &context.working_directory)?;
        let EntryData::Link(target) = &existing(namespace.nodes, &path, op)?.data else {
            return Err(rejected(op, Rejection::NotSymlink));
        };
        Ok(MemoryLink {
            shared: self.shared.clone(),
            path,
            target: target.clone(),
            observed: namespace.observed,
        })
    }

    pub(super) fn open_reader(
        &self,
        request: &ReadRequest,
        context: LookupContext,
    ) -> Result<MemoryFileReader, StorageError> {
        let op = Operation::Read;
        Self::supported_capabilities()
            .require_read(request)
            .map_err(|cause| rejected(op, cause))?;
        let mut nodes = self.lock(op)?;
        let path = select_with_context(&mut nodes, request.lookup(), &context, op)?;
        let EntryData::File(blob) = &existing(&nodes, &path, op)?.data else {
            return Err(rejected(op, Rejection::NotRegularFile));
        };
        Ok(MemoryFileReader::new(blob.clone(), request.range()))
    }

    pub(super) fn select_list_directory(
        &self,
        request: &ListRequest,
        context: LookupContext,
    ) -> Result<DirectorySelection, StorageError> {
        let op = Operation::List;
        let mut nodes = self.lock(op)?;
        let path = match request.target() {
            ListTarget::Path(_) => {
                let lookup = request
                    .lookup()
                    .ok_or_else(|| rejected(op, Rejection::Unsupported))?;
                select_with_context(&mut nodes, lookup, &context, op)?
            }
            ListTarget::Continue(cursor) => cursor.selection().address().clone(),
        };
        let identity = directory(&nodes, &path).ok_or_else(|| stale(op))?;
        let selection = DirectorySelection::from_lookup(path, **identity);
        request
            .bind_selection(selection.clone())
            .map_err(|_| stale(op))?;
        Ok(selection)
    }

    pub(super) fn enumerate_directory(
        &self,
        bound: &BoundListRequest<'_>,
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

    pub(super) fn create_directory(
        &self,
        request: &CreateDirectoryRequest,
        context: LookupContext,
    ) -> Result<Entry, StorageError> {
        let op = Operation::CreateDirectory;
        if !Self::supported_capabilities().directories().can_create() {
            return Err(rejected(op, Rejection::Unsupported));
        }
        let mut nodes = self.lock(op)?;
        let mut namespace = LookupNamespace::new(&mut nodes, op);
        let lookup = request.lookup();
        let directories = namespace.directories(lookup.path().start(), &context.working_directory)?;
        let result = Resolver::new(op, context.intermediate_symlinks)
            .walk(&mut namespace, lookup, directories)
            .and_then(|result| result.require_entry(op))
            .and_then(|entry| {
                if !namespace.created || request.mode() == CreateDirectoryMode::Recursive {
                    lookup
                        .check_existing_target(entry.path(), entry.entry_type())
                        .map_err(|reason| rejected(op, reason))?;
                }
                Ok(entry)
            });
        result.map_err(|error| {
            if namespace.created {
                StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
            } else {
                error
            }
        })
    }

    pub(super) fn remove(
        &self,
        request: &RemoveRequest,
        context: LookupContext,
    ) -> Result<(), StorageError> {
        let op = Operation::Remove;
        let mut nodes = self.lock(op)?;
        let path = select_with_context(&mut nodes, request.lookup(), &context, op)?;
        if request.kind() == RemoveKind::EmptyDirectory
            && nodes.keys().any(|key| descendant(key, path.as_str()))
        {
            return Err(rejected(op, Rejection::DirectoryNotEmpty));
        }
        nodes.remove(path.as_str());
        Ok(())
    }

    pub(super) fn rename(
        &self,
        request: &ah_vfs_storage::RenameRequest,
        context: LookupContext,
    ) -> Result<Entry, StorageError> {
        use ah_vfs_storage::{RenameDecision, RenameDestination};
        let op = Operation::Rename;
        let mut nodes = self.lock(op)?;
        let (source_lookup, destination_lookup) = request.lookups();
        let base = &context.working_directory;
        if [source_lookup, destination_lookup]
            .iter()
            .any(|lookup| lookup.path().start() == LookupStart::BaseDirectory)
            && directory(&nodes, base).is_none()
        {
            return Err(stale(op));
        }
        let mut namespace = LookupNamespace::new(&mut nodes, op);
        let mut resolver = Resolver::new(op, context.intermediate_symlinks);
        let source_path = namespace.select(&mut resolver, source_lookup, base)?;
        let directories = namespace.directories(destination_lookup.path().start(), base)?;
        let destination_path = match resolver.walk(&mut namespace, destination_lookup, directories)? {
            Resolution::Existing(entry) => entry.path().clone(),
            Resolution::Missing(address) => address,
        };
        let source = existing(&nodes, &source_path, op)?;
        let slot = |address: &StoragePath| -> Result<_, StorageError> {
            let parent = address
                .parent()
                .ok_or_else(|| rejected(op, Rejection::RootProtected))?;
            directory(&nodes, &parent).ok_or_else(|| stale(op))
        };
        let destination = if source_path.name() == destination_path.name()
            && Arc::ptr_eq(slot(&source_path)?, slot(&destination_path)?)
        {
            RenameDestination::SameSlot
        } else if let Some(entry) = nodes.get(destination_path.as_str()) {
            let kind = entry.entry_type();
            if kind == EntryType::Directory
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
        let decision = request
            .decide_destination(&destination_path, source.entry_type(), destination)
            .map_err(|reason| rejected(op, reason))?;
        let result = metadata(destination_path.clone(), source, op)?;
        if decision == RenameDecision::Unchanged {
            return Ok(result);
        }
        if descendant(destination_path.as_str(), source_path.as_str()) {
            return Err(rejected(op, Rejection::Conflict));
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

    pub(super) fn open_writer(
        &self,
        request: &WriteRequest,
        context: LookupContext,
    ) -> Result<MemoryFileWriter, StorageError> {
        let op = Operation::Write;
        Self::supported_capabilities()
            .require_write(request)
            .map_err(|cause| rejected(op, cause))?;
        let mut nodes = self.lock(op)?;
        let path = select_with_context(&mut nodes, request.lookup(), &context, op)?;
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
        Ok(MemoryFileWriter::new(
            self.shared.clone(),
            path,
            parent_path,
            parent,
            destination,
        ))
    }
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
fn metadata(path: StoragePath, entry: &MemoryEntry, op: Operation) -> Result<Entry, StorageError> {
    entry
        .metadata(path)
        .map_err(|cause| invalid(op, ContractViolation::InvalidEntry, cause))
}

fn select_with_context(
    nodes: &mut Nodes,
    lookup: PathLookup<'_>,
    context: &LookupContext,
    op: Operation,
) -> Result<StoragePath, StorageError> {
    // Hold the namespace lock from the first observation through the effect.
    let mut resolver = Resolver::new(op, context.intermediate_symlinks);
    LookupNamespace::new(nodes, op).select(&mut resolver, lookup, &context.working_directory)
}

struct LookupNamespace<'a> {
    nodes: &'a mut Nodes,
    operation: Operation,
    created: bool,
    observed: BTreeMap<StoragePath, EntryData>,
}

impl<'a> LookupNamespace<'a> {
    fn new(nodes: &'a mut Nodes, operation: Operation) -> Self {
        Self {
            nodes,
            operation,
            created: false,
            observed: BTreeMap::new(),
        }
    }

    fn directories(&mut self, start: LookupStart, base: &StoragePath) -> Result<Vec<Entry>, StorageError> {
        let mut address = Some(match start {
            LookupStart::InstanceRoot => StoragePath::root(),
            LookupStart::BaseDirectory => base.clone(),
        });
        let mut chain = Vec::new();
        while let Some(path) = address {
            let Some(node) = self.nodes.get(path.as_str()) else {
                return Err(stale(self.operation));
            };
            if node.entry_type() != EntryType::Directory {
                return Err(stale(self.operation));
            }
            address = path.parent();
            chain.push(self.observe_at(path)?);
        }
        chain.reverse();
        Ok(chain)
    }

    fn observe_at(&mut self, address: StoragePath) -> Result<Entry, StorageError> {
        let node = existing(self.nodes, &address, self.operation)?;
        if self.operation == Operation::ReadLink {
            self.observed.insert(address.clone(), node.data.clone());
        }
        metadata(address, node, self.operation)
    }

    fn select(
        &mut self,
        resolver: &mut Resolver,
        lookup: PathLookup<'_>,
        base: &StoragePath,
    ) -> Result<StoragePath, StorageError> {
        let op = self.operation;
        let directories = self.directories(lookup.path().start(), base)?;
        match resolver.walk(self, lookup, directories)? {
            Resolution::Existing(entry) => {
                lookup
                    .check_existing_target(entry.path(), entry.entry_type())
                    .map_err(|cause| rejected(op, cause))?;
                Ok(entry.path().clone())
            }
            Resolution::Missing(address) => Ok(address),
        }
    }
}

impl LookupPort for LookupNamespace<'_> {
    type Binding = Entry;

    fn entry(binding: &Entry) -> &Entry {
        binding
    }

    fn observe_child(
        &mut self,
        _parent: &Entry,
        address: StoragePath,
        _name: &EntryName,
    ) -> Result<Entry, StorageError> {
        self.observe_at(address)
    }

    fn read_bound_link(&self, entry: &Entry) -> Result<LinkTarget, StorageError> {
        match &existing(self.nodes, entry.path(), self.operation)?.data {
            EntryData::Link(target) => Ok(target.as_ref().clone()),
            _ => Err(rejected(self.operation, Rejection::NotSymlink)),
        }
    }

    fn create_child(
        &mut self,
        _parent: &Entry,
        address: StoragePath,
        _name: &EntryName,
    ) -> Result<Entry, StorageError> {
        let MapEntry::Vacant(slot) = self.nodes.entry(address.as_str().to_owned()) else {
            return Err(rejected(self.operation, Rejection::AlreadyExists));
        };
        self.created = true;
        metadata(address, slot.insert(MemoryEntry::dir()), self.operation)
    }
}

pub(super) struct MemoryLink {
    shared: Arc<Shared>,
    path: StoragePath,
    target: Arc<LinkTarget>,
    observed: BTreeMap<StoragePath, EntryData>,
}

impl BoundLink for MemoryLink {
    fn path(&self) -> &StoragePath {
        &self.path
    }

    fn read(&self, request: ReadLinkRequest) -> StorageFuture<'_, LinkTarget> {
        Box::pin(async move {
            let op = Operation::ReadLink;
            if request.path() != &self.path {
                return Err(StorageError::without_effects(
                    op,
                    Failure::Contract(ContractViolation::InvalidBinding),
                    None,
                ));
            }
            let nodes = self.shared.lock(op)?;
            for (path, observed) in &self.observed {
                let matches = nodes.get(path.as_str()).is_some_and(|entry| {
                    match (&entry.data, observed) {
                        (EntryData::Dir(left), EntryData::Dir(right)) => Arc::ptr_eq(left, right),
                        (EntryData::File(left), EntryData::File(right)) => Arc::ptr_eq(left, right),
                        (EntryData::Link(left), EntryData::Link(right)) => Arc::ptr_eq(left, right),
                        _ => false,
                    }
                });
                if !matches {
                    return Err(rejected(op, Rejection::Conflict));
                }
            }
            Ok(self.target.as_ref().clone())
        })
    }
}
