//! A bounded storage-contract entry point on the memory driver's native state.
use super::{
    CHUNK_SIZE, EntryData, MemoryEntry, MemoryStorage, Shared, children, descendant,
    file_reader::MemoryFileReader, file_writer::MemoryFileWriter,
};
use crate::Capacity;
use ah_vfs::application::port::LookupContext;
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
                data: EntryData::Link(target),
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

    pub(super) fn read_link_selected(
        &self,
        request: &ReadLinkRequest,
        context: LookupContext,
    ) -> Result<LinkTarget, StorageError> {
        let op = Operation::ReadLink;
        let mut nodes = self.lock(op)?;
        let path = select_with_context(&mut nodes, request.lookup(), &context, op)?;
        match &existing(&nodes, &path, op)?.data {
            EntryData::Link(target) => Ok(target.clone()),
            _ => Err(rejected(op, Rejection::NotSymlink)),
        }
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
        if request.path().start() == LookupStart::BaseDirectory
            && directory(&nodes, &context.working_directory).is_none()
        {
            return Err(stale(op));
        }
        let mut walk = Walk::new(&mut nodes, request.lookup(), context.intermediate_symlinks);
        let path = walk
            .start_at(&context.working_directory)
            .map_err(|failure| {
                if walk.created {
                    StorageError::after_dispatch(op, failure, None)
                } else {
                    StorageError::without_effects(op, failure, None)
                }
            })?;
        let mut created = walk.created;
        let entry = nodes.entry(path.as_str().into()).or_insert_with(|| {
            created = true;
            MemoryEntry::dir()
        });
        metadata(path, entry, op).map_err(|error| {
            if created {
                StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
            } else {
                error
            }
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

fn select_with_context(
    nodes: &mut Nodes,
    lookup: PathLookup<'_>,
    context: &LookupContext,
    op: Operation,
) -> Result<StoragePath, StorageError> {
    if lookup.path().start() == LookupStart::BaseDirectory
        && directory(nodes, &context.working_directory).is_none()
    {
        return Err(stale(op));
    }
    // The namespace lock binds the base and selected entry until the caller
    // has obtained metadata or copied the link bytes.
    Walk::new(nodes, lookup, context.intermediate_symlinks)
        .start_at(&context.working_directory)
        .map_err(|failure| StorageError::without_effects(op, failure, None))
}

struct Walk<'a, 'r> {
    nodes: &'a mut Nodes,
    lookup: PathLookup<'r>,
    links: SymlinkTraversal,
    created: bool,
}

impl<'a, 'r> Walk<'a, 'r> {
    fn new(nodes: &'a mut Nodes, lookup: PathLookup<'r>, links: IntermediateSymlinks) -> Self {
        Self {
            nodes,
            lookup,
            links: SymlinkTraversal::new(links),
            created: false,
        }
    }

    fn start(&mut self) -> Result<StoragePath, Failure> {
        self.start_at(&StoragePath::root())
    }

    fn start_at(&mut self, base: &StoragePath) -> Result<StoragePath, Failure> {
        let path = self.segment(self.lookup.path(), base, &[], false)?;
        if let Some(entry) = self.nodes.get(path.as_str()) {
            self.lookup
                .check_existing_target(&path, entry.entry_type())
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
                            if missing == MissingName::OperandIntermediate {
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
            expr.check_directory_requirement(entry.entry_type() == EntryType::Directory)
                .map_err(Failure::Rejected)?;
        }
        Ok(path)
    }
}
