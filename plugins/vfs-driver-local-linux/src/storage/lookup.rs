//! Handle-bound metadata and link lookup on Linux.
use super::{attributes, blocking, file_reader::LocalFileReader, file_writer::LocalFileWriter, fs};
use ah_vfs::application::port::{
    DirectoryBindings, EntrySlot, LookupScope, ObservedEntry, OperationLookup, StorageFileReader,
    StorageFileWriter, StorageFuture,
};
use ah_vfs_storage::{
    AttributeRequest, AttributeResult, BackendFailure, ContractViolation, CreateDirectoryRequest,
    DirectorySelection, Entry, EntryName, EntryType, Failure, LinkTarget, ListRequest, Operation,
    ReadLinkRequest, ReadRequest, Rejection, RemoveKind, RemoveRequest, RenameDestination,
    RenameMode, RenameRequest, StatRequest, StorageError, StoragePath, WriteRequest,
};
use cap_std::fs::{Dir, File};
use rustix::fs::{Mode, OFlags, ResolveFlags, fstat, mkdirat, openat2, readlinkat};
use rustix::io::Errno;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

fn failure(operation: Operation, reason: Failure) -> StorageError {
    StorageError::without_effects(operation, reason, None)
}

fn rejected(operation: Operation, reason: Rejection) -> StorageError {
    failure(operation, Failure::Rejected(reason))
}

fn stale(operation: Operation) -> StorageError {
    failure(operation, Failure::Backend(BackendFailure::Stale))
}

#[derive(Clone)]
struct Selected {
    entry: Entry,
    file: Arc<File>,
}

pub(super) struct LocalLookup {
    root: Arc<Dir>,
    operation: Operation,
    directories: Arc<DirectoryBindings<Dir>>,
    missing: BTreeSet<StoragePath>,
    scope: LookupScope,
    observed: Arc<Mutex<BTreeMap<StoragePath, Arc<File>>>>,
}

impl LocalLookup {
    pub(super) fn new(
        root: Arc<Dir>,
        directories: Arc<DirectoryBindings<Dir>>,
        operation: Operation,
    ) -> Self {
        Self {
            root,
            directories,
            operation,
            missing: BTreeSet::new(),
            scope: LookupScope::default(),
            observed: Arc::default(),
        }
    }

    fn selected(&self, entry: &ObservedEntry) -> Result<Selected, StorageError> {
        self.scope
            .get::<Selected>(entry)
            .cloned()
            .ok_or_else(|| stale(self.operation))
    }

    fn bound_slot(
        &self,
        op: Operation,
        slot: EntrySlot<'_>,
        path: &StoragePath,
    ) -> Result<Slot, StorageError> {
        slot.validate(op, path)?;
        let parent = self.selected(slot.parent)?;
        let entry = slot.entry.map(|entry| self.selected(entry)).transpose()?;
        if self.operation != op || (entry.is_none() && !self.missing.contains(path)) {
            return Err(failure(op, Failure::InvalidBinding));
        }
        Ok(Slot {
            parent,
            name: slot.name.clone(),
            path: path.clone(),
            entry,
        })
    }

    fn bound_entry(
        &self,
        op: Operation,
        slot: Option<EntrySlot<'_>>,
        entry: &ObservedEntry,
        address: &StoragePath,
    ) -> Result<Selected, StorageError> {
        let selected = self.selected(entry)?;
        if self.operation != op
            || selected.entry.path() != address
            || slot.is_none() != address.is_root()
        {
            return Err(failure(op, Failure::InvalidBinding));
        }
        if let Some(slot) = slot {
            let expected = self.bound_slot(op, slot, address)?.require_entry(op)?;
            if !Arc::ptr_eq(&expected.file, &selected.file) {
                return Err(failure(op, Failure::InvalidBinding));
            }
        }
        Ok(selected)
    }
}

impl OperationLookup for LocalLookup {
    fn operation(&self) -> Operation {
        self.operation
    }

    fn root(&mut self) -> StorageFuture<'_, ObservedEntry> {
        let root = self.root.clone();
        Box::pin(async move {
            let op = self.operation();
            let selected = blocking(op, false, move || {
                let path = StoragePath::root();
                let file = open_observed(&root, &path, op)?;
                observe_selected(op, &mut BTreeMap::new(), path, file)
            })
            .await?;
            Ok(self.scope.bind(selected.entry.clone(), selected))
        })
    }

    fn observe<'a>(
        &'a mut self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            let op = self.operation();
            let parent = self
                .scope
                .get::<Selected>(parent)
                .ok_or_else(|| stale(op))?
                .clone();
            if parent.entry.entry_type() != EntryType::Directory {
                return Err(rejected(op, Rejection::NotDirectory));
            }
            let path = parent
                .entry
                .path()
                .join(name)
                .map_err(|_| rejected(op, Rejection::PathTooLong))?;
            let name = name.as_str().to_owned();
            let observed = self.observed.clone();
            let address = path.clone();
            let selected = blocking(op, false, move || {
                let file = open_child(op, &parent, &name)?;
                let mut observed = observed
                    .lock()
                    .map_err(|_| failure(op, Failure::Backend(BackendFailure::Other)))?;
                observe_selected(op, &mut observed, path, file)
            })
            .await;
            let selected = match selected {
                Ok(_) if self.missing.contains(&address) => {
                    // A concurrent directory may win mkdirat. The resolver may
                    // re-observe it for recursive creation after that rejection.
                    return Err(rejected(op, Rejection::Conflict));
                }
                Ok(selected) => selected,
                Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => {
                    if self
                        .observed
                        .lock()
                        .map_err(|_| failure(op, Failure::Backend(BackendFailure::Other)))?
                        .contains_key(&address)
                    {
                        return Err(rejected(op, Rejection::Conflict));
                    }
                    self.missing.insert(address);
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            Ok(self.scope.bind(selected.entry.clone(), selected))
        })
    }

    fn read_link<'a>(&'a self, entry: &'a ObservedEntry) -> StorageFuture<'a, LinkTarget> {
        Box::pin(async move {
            let op = self.operation();
            let selected = self.scope.get::<Selected>(entry).ok_or_else(|| stale(op))?;
            if selected.entry.entry_type() != EntryType::Symlink {
                return Err(rejected(op, Rejection::NotSymlink));
            }
            let file = selected.file.clone();
            blocking(op, false, move || link_bytes(op, &file)).await
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
            let slot = EntrySlot {
                parent,
                name,
                entry: Some(link),
            };
            let selected = self
                .bound_slot(op, slot, request.path())?
                .require_entry(op)?;
            request
                .check_target(selected.entry.entry_type())
                .map_err(|reason| rejected(op, reason))?;
            self.verify().await?;
            blocking(op, false, move || link_bytes(op, &selected.file)).await
        })
    }

    fn execute_stat<'a>(
        &'a self,
        slot: Option<EntrySlot<'a>>,
        entry: &'a ObservedEntry,
        request: StatRequest,
    ) -> StorageFuture<'a, Entry> {
        Box::pin(async move {
            let entry = self.bound_entry(Operation::Stat, slot, entry, request.path())?;
            self.verify().await?;
            Ok(entry.entry)
        })
    }

    fn execute_read<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: ReadRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            let op = Operation::Read;
            let entry = self
                .bound_slot(op, slot, request.path())?
                .require_entry(op)?;
            request
                .check_target(entry.entry.entry_type())
                .map_err(|reason| rejected(op, reason))?;
            self.verify().await?;
            blocking(op, false, move || {
                let file = open_reader(&entry)?;
                Ok(Box::new(LocalFileReader::open(file, request.range())?)
                    as Box<dyn StorageFileReader>)
            })
            .await
        })
    }

    fn execute_write<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: WriteRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            let op = Operation::Write;
            let slot = self.bound_slot(op, slot, request.path())?;
            self.verify().await?;
            request
                .check_target(slot.entry.as_ref().map(|entry| entry.entry.entry_type()))
                .map_err(|reason| rejected(op, reason))?;
            blocking(op, false, move || {
                slot.verify_absence(op)?;
                let parent = clone_directory(op, &slot.parent.file)?;
                Ok(Box::new(LocalFileWriter::open(parent, request)?) as Box<dyn StorageFileWriter>)
            })
            .await
        })
    }

    fn execute_create_directory<'a>(
        &'a mut self,
        slot: EntrySlot<'a>,
        request: CreateDirectoryRequest,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            let op = Operation::CreateDirectory;
            let slot = self.bound_slot(op, slot, request.path())?;
            request
                .check_target(slot.entry.as_ref().map(|entry| entry.entry.entry_type()))
                .map_err(|reason| rejected(op, reason))?;
            self.verify().await?;
            self.missing.remove(request.path());
            let observed = self.observed.clone();
            let selected = blocking(op, true, move || create_child(&slot, &observed)).await?;
            Ok(self.scope.bind(selected.entry.clone(), selected))
        })
    }

    fn execute_remove<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: RemoveRequest,
    ) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let op = Operation::Remove;
            let slot = self.bound_slot(op, slot, request.path())?;
            let entry = slot.require_entry(op)?;
            request
                .check_target(entry.entry.entry_type())
                .map_err(|reason| rejected(op, reason))?;
            self.verify().await?;
            blocking(op, true, move || {
                use rustix::fs::{AtFlags, unlinkat};
                let flags = match request.kind() {
                    RemoveKind::NonDirectory => AtFlags::empty(),
                    RemoveKind::EmptyDirectory => AtFlags::REMOVEDIR,
                };
                // The parent is pinned. The slot may change after verification.
                unlinkat(slot.parent.file.as_ref(), slot.name.as_str(), flags)
                    .map_err(|cause| mutation_error(op, cause))
            })
            .await
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
            let source = self.bound_slot(op, source, request.source())?;
            let destination = self.bound_slot(op, destination, request.destination())?;
            let root = self.root.clone();
            let observed = self.observed.clone();
            blocking(op, true, move || {
                rename(&request, &source, &destination, || {
                    let observed = lock_observations(&observed, op)?;
                    verify_observations(&root, &observed, op)
                })
            })
            .await
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
            let entry = self.bound_entry(op, slot, entry, request.directory())?;
            if request.after().is_some() || entry.entry.entry_type() != EntryType::Directory {
                return Err(failure(op, Failure::InvalidBinding));
            }
            self.verify().await?;
            let directories = self.directories.clone();
            blocking(op, false, move || {
                let dir = clone_directory(op, &entry.file)?;
                directories.bind(request.directory().clone(), dir, same_directory)
            })
            .await
        })
    }

    fn execute_attribute<'a>(
        &'a self,
        slot: Option<(&'a ObservedEntry, &'a EntryName)>,
        entry: &'a ObservedEntry,
        request: AttributeRequest,
    ) -> StorageFuture<'a, AttributeResult> {
        Box::pin(async move {
            let op = request.operation();
            let selected = self
                .scope
                .get::<Selected>(entry)
                .ok_or_else(|| stale(op))?
                .clone();
            let matches_slot = if let Some((parent, name)) = slot {
                let parent = self
                    .scope
                    .get::<Selected>(parent)
                    .ok_or_else(|| stale(op))?;
                parent.entry.entry_type() == EntryType::Directory
                    && parent.entry.path().join(name).as_ref() == Ok(request.path())
            } else {
                request.path().is_root()
            };
            if op != self.operation || selected.entry.path() != request.path() || !matches_slot {
                return Err(failure(op, Failure::InvalidBinding));
            }
            self.verify().await?;
            blocking(op, op.may_mutate(), move || {
                attributes::execute(&selected.entry, &selected.file, request)
            })
            .await
        })
    }

    fn verify(&self) -> StorageFuture<'_, ()> {
        let root = self.root.clone();
        let observed = self.observed.clone();
        let op = self.operation();
        Box::pin(blocking(op, false, move || {
            let observed = observed
                .lock()
                .map_err(|_| failure(op, Failure::Backend(BackendFailure::Other)))?;
            verify_observations(&root, &observed, op)
        }))
    }
}

pub(super) fn descriptor_path(file: &File) -> String {
    use std::os::fd::AsRawFd;
    format!("/proc/self/fd/{}", file.as_raw_fd())
}

fn open_reader(selected: &Selected) -> Result<File, StorageError> {
    let op = Operation::Read;
    // The procfs descriptor refers to the pinned inode even after a rename.
    // Reopening the selected name would race a replacement with a different file.
    let descriptor = descriptor_path(&selected.file);
    let fd = rustix::fs::open(
        descriptor.as_str(),
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|cause| match cause {
        Errno::NOENT | Errno::NOTDIR => rejected(op, Rejection::Unsupported),
        _ => fs::error(op, cause.into()),
    })?;
    let file = File::from_std(fd.into());
    if identity(op, &selected.file)? != identity(op, &file)? {
        return Err(failure(
            op,
            Failure::Contract(ContractViolation::InvalidEntry),
        ));
    }
    Ok(file)
}

struct Slot {
    parent: Selected,
    name: EntryName,
    path: StoragePath,
    entry: Option<Selected>,
}

impl Slot {
    fn require_entry(&self, op: Operation) -> Result<Selected, StorageError> {
        self.entry
            .clone()
            .ok_or_else(|| failure(op, Failure::InvalidBinding))
    }

    fn verify_absence(&self, op: Operation) -> Result<(), StorageError> {
        if self.entry.is_some() {
            return Ok(());
        }
        match open_child(op, &self.parent, self.name.as_str()) {
            Ok(_) => Err(rejected(op, Rejection::Conflict)),
            Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn lock_observations(
    observed: &Mutex<BTreeMap<StoragePath, Arc<File>>>,
    op: Operation,
) -> Result<std::sync::MutexGuard<'_, BTreeMap<StoragePath, Arc<File>>>, StorageError> {
    observed
        .lock()
        .map_err(|_| failure(op, Failure::Backend(BackendFailure::Other)))
}

fn rename(
    request: &RenameRequest,
    source: &Slot,
    destination: &Slot,
    verify: impl FnOnce() -> Result<(), StorageError>,
) -> Result<Entry, StorageError> {
    use rustix::fs::{RenameFlags, renameat_with};
    let op = Operation::Rename;
    let selected = source.require_entry(op)?;
    let kind = selected.entry.entry_type();
    let same_slot = source.name == destination.name
        && identity(op, &source.parent.file)? == identity(op, &destination.parent.file)?;
    let observed = match &destination.entry {
        None => RenameDestination::Missing,
        Some(target)
            if !same_slot
                && target.entry.entry_type() == EntryType::Directory
                && request.mode() == RenameMode::Replace
                && kind == EntryType::Directory
                && !empty_directory(&target.file)? =>
        {
            RenameDestination::NonEmptyDirectory
        }
        Some(target) => RenameDestination::Occupied(target.entry.entry_type()),
    };
    request
        .decide_destination(kind, observed)
        .map_err(|reason| rejected(op, reason))?;
    let result =
        Entry::new(destination.path.clone(), kind, selected.entry.modified()).map_err(|cause| {
            StorageError::without_effects(
                op,
                Failure::Contract(ContractViolation::InvalidEntry),
                Some(Box::new(cause)),
            )
        })?;
    verify()?;
    if same_slot {
        return Ok(result);
    }
    // ParentPinned verifies both slots before dispatch. NOREPLACE checks
    // destination absence atomically. Other checked names can change before
    // dispatch, and rename can then move an entry of a different type.
    let flags = if observed == RenameDestination::Missing {
        RenameFlags::NOREPLACE
    } else {
        RenameFlags::empty()
    };
    renameat_with(
        source.parent.file.as_ref(),
        source.name.as_str(),
        destination.parent.file.as_ref(),
        destination.name.as_str(),
        flags,
    )
    .map_err(|cause| {
        if cause == Errno::EXIST && request.mode() == RenameMode::Replace {
            rejected(op, Rejection::Conflict)
        } else {
            mutation_error(op, cause)
        }
    })?;
    let published =
        open_child(op, &destination.parent, destination.name.as_str()).and_then(|file| {
            if identity(op, &selected.file)? == identity(op, &file)? {
                Ok(())
            } else {
                Err(failure(op, Failure::Backend(BackendFailure::Other)))
            }
        });
    published.map_err(|error| {
        StorageError::after_dispatch(
            op,
            Failure::Backend(BackendFailure::Other),
            Some(Box::new(error)),
        )
    })?;
    Ok(result)
}

fn empty_directory(file: &File) -> Result<bool, StorageError> {
    let op = Operation::Rename;
    let fd = openat2(
        file,
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH,
    )
    .map_err(|cause| fs::error(op, cause.into()))?;
    let dir = Dir::from_std_file(fd.into());
    Ok(dir
        .entries()
        .map_err(|cause| fs::error(op, cause))?
        .next()
        .transpose()
        .map_err(|cause| fs::error(op, cause))?
        .is_none())
}

fn mutation_error(op: Operation, cause: Errno) -> StorageError {
    match cause {
        Errno::NOSYS | Errno::OPNOTSUPP => rejected(op, Rejection::Unsupported),
        Errno::ACCESS
        | Errno::PERM
        | Errno::EXIST
        | Errno::NOENT
        | Errno::NOTDIR
        | Errno::ISDIR
        | Errno::NOTEMPTY
        | Errno::XDEV
        | Errno::ROFS
        | Errno::BUSY
        | Errno::NAMETOOLONG
        | Errno::NOSPC
        | Errno::DQUOT => fs::error(op, cause.into()),
        _ => fs::dispatched_error(op, cause.into()),
    }
}

fn link_bytes(op: Operation, file: &File) -> Result<LinkTarget, StorageError> {
    // Linux accepts an empty path for an O_PATH descriptor of the link itself.
    // Reading by descriptor keeps the bytes and the classified type on one object.
    readlinkat(file, "", Vec::new())
        .map(|bytes| LinkTarget::new(bytes.into_bytes()))
        .map_err(|cause| fs::error(op, cause.into()))
}

fn create_child(
    slot: &Slot,
    observed: &Mutex<BTreeMap<StoragePath, Arc<File>>>,
) -> Result<Selected, StorageError> {
    let op = Operation::CreateDirectory;
    let mode = Mode::RUSR
        | Mode::WUSR
        | Mode::XUSR
        | Mode::RGRP
        | Mode::WGRP
        | Mode::XGRP
        | Mode::ROTH
        | Mode::WOTH
        | Mode::XOTH;
    match mkdirat(slot.parent.file.as_ref(), slot.name.as_str(), mode) {
        Ok(()) => {
            let selected = (|| {
                let file = open_child(op, &slot.parent, slot.name.as_str())?;
                let mut observed = lock_observations(observed, op)?;
                let selected = observe_selected(op, &mut observed, slot.path.clone(), file)?;
                if selected.entry.entry_type() != EntryType::Directory {
                    return Err(failure(op, Failure::Backend(BackendFailure::Other)));
                }
                Ok(selected)
            })();
            selected.map_err(|error| {
                StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
            })
        }
        Err(Errno::EXIST) => Err(rejected(op, Rejection::AlreadyExists)),
        Err(
            cause @ (Errno::NOENT
            | Errno::NOTDIR
            | Errno::ACCESS
            | Errno::PERM
            | Errno::ROFS
            | Errno::NAMETOOLONG
            | Errno::NOSPC
            | Errno::DQUOT
            | Errno::INVAL
            | Errno::LOOP),
        ) => Err(fs::error(op, cause.into())),
        Err(cause) => Err(fs::dispatched_error(op, cause.into())),
    }
}

fn open_child(op: Operation, parent: &Selected, name: &str) -> Result<File, StorageError> {
    let fd = openat2(
        parent.file.as_ref(),
        name,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|cause| fs::error(op, cause.into()))?;
    Ok(File::from_std(fd.into()))
}

fn observe_selected(
    op: Operation,
    observed: &mut BTreeMap<StoragePath, Arc<File>>,
    path: StoragePath,
    file: File,
) -> Result<Selected, StorageError> {
    let metadata = file.metadata().map_err(|cause| fs::error(op, cause))?;
    let entry = fs::entry(op, path, metadata)?;
    let file = Arc::new(file);
    let file = if entry.path().is_root() {
        file
    } else {
        match observed.entry(entry.path().clone()) {
            std::collections::btree_map::Entry::Vacant(slot) => slot.insert(file).clone(),
            std::collections::btree_map::Entry::Occupied(slot) => {
                if identity(op, slot.get())? != identity(op, &file)? {
                    return Err(rejected(op, Rejection::Conflict));
                }
                slot.get().clone()
            }
        }
    };
    Ok(Selected { entry, file })
}

fn open_observed(
    root: &Dir,
    path: &StoragePath,
    operation: Operation,
) -> Result<File, StorageError> {
    let name = if path.is_root() { "." } else { path.as_str() };
    openat2(
        root,
        name,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .map(|fd| File::from_std(fd.into()))
    .map_err(|cause| match cause {
        Errno::NOSYS => rejected(operation, Rejection::Unsupported),
        Errno::AGAIN | Errno::LOOP => rejected(operation, Rejection::Conflict),
        _ => fs::error(operation, cause.into()),
    })
}

fn verify_observations(
    root: &Dir,
    observed: &BTreeMap<StoragePath, Arc<File>>,
    operation: Operation,
) -> Result<(), StorageError> {
    for (path, bound) in observed {
        let current =
            open_observed(root, path, operation).map_err(|error| match error.failure() {
                Failure::Rejected(Rejection::NotFound | Rejection::NotDirectory) => {
                    rejected(operation, Rejection::Conflict)
                }
                _ => error,
            })?;
        if identity(operation, bound)? != identity(operation, &current)? {
            return Err(rejected(operation, Rejection::Conflict));
        }
    }
    Ok(())
}

fn identity(op: Operation, file: &File) -> Result<impl Eq, StorageError> {
    fstat(file)
        .map(|stat| (stat.st_dev, stat.st_ino))
        .map_err(|cause| fs::error(op, cause.into()))
}

fn clone_directory(op: Operation, file: &File) -> Result<Dir, StorageError> {
    Ok(Dir::from_std_file(
        file.try_clone()
            .map_err(|cause| fs::error(op, cause))?
            .into_std(),
    ))
}

pub(super) fn same_directory(left: &Dir, right: &Dir) -> Result<bool, StorageError> {
    let stat = |dir: &Dir| fstat(dir).map_err(|cause| fs::error(Operation::List, cause.into()));
    let (left, right) = (stat(left)?, stat(right)?);
    Ok(left.st_nlink != 0
        && right.st_nlink != 0
        && (left.st_dev, left.st_ino) == (right.st_dev, right.st_ino))
}

pub(super) fn list_stale() -> StorageError {
    stale(Operation::List)
}

pub(super) fn list_replaced(resuming: bool) -> StorageError {
    if resuming {
        list_stale()
    } else {
        rejected(Operation::List, Rejection::Conflict)
    }
}

pub(super) fn reopen_listing_directory(
    root: &Dir,
    selection: &ah_vfs_storage::DirectorySelection,
    resuming: bool,
) -> Result<Dir, StorageError> {
    let op = Operation::List;
    let file =
        open_observed(root, selection.address(), op).map_err(|error| match error.failure() {
            Failure::Rejected(
                Rejection::NotFound | Rejection::NotDirectory | Rejection::Conflict,
            ) => list_replaced(resuming),
            Failure::Rejected(Rejection::Unsupported) if resuming => list_stale(),
            _ => error,
        })?;
    if !file
        .metadata()
        .map_err(|cause| fs::error(op, cause))?
        .is_dir()
    {
        return Err(list_replaced(resuming));
    }
    Ok(Dir::from_std_file(file.into_std()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cross_device_rename_leaves_both_slots_unchanged()
    -> Result<(), Box<dyn std::error::Error>> {
        use ah_vfs_storage::{MutationOutcome, RenameMode};
        use std::os::unix::fs::MetadataExt;

        let source = tempfile::tempdir()?;
        let Ok(destination) = tempfile::tempdir_in("/dev/shm") else {
            return Ok(());
        };
        if source.path().metadata()?.dev() == destination.path().metadata()?.dev() {
            return Ok(());
        }
        let from = source.path().join("file");
        let to = destination.path().join("file");
        std::fs::write(&from, b"original")?;
        let root = Dir::open_ambient_dir("/", cap_std::ambient_authority())?;
        let storage = super::super::LinuxLocalStorage {
            root: "/".into(),
            dir: Arc::new(root),
            directory_bindings: Arc::default(),
        };
        use ah_vfs::resolve::{StorageOperations, operand::RenameOperand, path::PathExpr};
        let request = RenameOperand::new(
            PathExpr::parse(from.to_str().ok_or("source")?)?,
            PathExpr::parse(to.to_str().ok_or("destination")?)?,
            RenameMode::NoReplace,
        )?;
        let error = storage
            .move_entry(request, Default::default())
            .await
            .err()
            .ok_or("cross-device rename succeeded")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::CrossDevice));
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
        assert_eq!(std::fs::read(from)?, b"original");
        assert!(!to.exists());
        Ok(())
    }

    #[tokio::test]
    async fn replacing_a_bound_link_cannot_supply_new_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::os::unix::fs::symlink("before", directory.path().join("link"))?;
        let root = Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority())?;
        let mut lookup = LocalLookup::new(Arc::new(root), Arc::default(), Operation::ReadLink);
        let root = lookup.root().await?;
        let name = EntryName::parse("link")?;
        let link = lookup.observe(&root, &name).await?;

        std::fs::rename(directory.path().join("link"), directory.path().join("old"))?;
        std::os::unix::fs::symlink("after", directory.path().join("link"))?;

        assert_eq!(lookup.read_link(&link).await?.as_bytes(), b"before");
        let error = lookup
            .execute_read_link(
                &root,
                &name,
                &link,
                ah_vfs_storage::ReadLinkRequest::new(link.entry().path().clone()),
            )
            .await
            .err()
            .ok_or("replaced link passed verification")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::Conflict));
        assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unchanged);
        Ok(())
    }

    #[tokio::test]
    async fn replaced_slots_fail_before_unlink() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let root = Arc::new(Dir::open_ambient_dir(
            directory.path(),
            cap_std::ambient_authority(),
        )?);
        for kind in [RemoveKind::NonDirectory, RemoveKind::EmptyDirectory] {
            let name = EntryName::parse("target")?;
            let path = StoragePath::parse("target")?;
            let target = directory.path().join(name.as_str());
            let old = directory.path().join("old");
            match kind {
                RemoveKind::NonDirectory => std::fs::write(&target, b"first")?,
                RemoveKind::EmptyDirectory => std::fs::create_dir(&target)?,
            }
            let mut lookup = LocalLookup::new(root.clone(), Arc::default(), Operation::Remove);
            let parent = lookup.root().await?;
            let entry = lookup.observe(&parent, &name).await?;
            std::fs::rename(&target, &old)?;
            match kind {
                RemoveKind::NonDirectory => std::fs::write(&target, b"second")?,
                RemoveKind::EmptyDirectory => std::fs::create_dir(&target)?,
            }
            let slot = EntrySlot {
                parent: &parent,
                name: &name,
                entry: Some(&entry),
            };
            let error = lookup
                .execute_remove(slot, RemoveRequest::new(path, kind)?)
                .await
                .err()
                .ok_or("replaced slot passed verification")?;
            assert_eq!(error.failure(), Failure::Rejected(Rejection::Conflict));
            assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unchanged);
            assert!(target.exists());
            assert!(old.exists());
            match kind {
                RemoveKind::NonDirectory => {
                    std::fs::remove_file(&target)?;
                    std::fs::remove_file(&old)?;
                }
                RemoveKind::EmptyDirectory => {
                    std::fs::remove_dir(&target)?;
                    std::fs::remove_dir(&old)?;
                }
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn replacing_a_bound_parent_with_a_link_conflicts_before_unlink()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let base = directory.path();
        std::fs::create_dir(base.join("parent"))?;
        std::fs::create_dir(base.join("other"))?;
        std::fs::write(base.join("parent/file"), b"observed")?;
        std::fs::write(base.join("other/file"), b"referent")?;
        let root = Arc::new(Dir::open_ambient_dir(base, cap_std::ambient_authority())?);
        let mut lookup = LocalLookup::new(root, Arc::default(), Operation::Remove);
        let root = lookup.root().await?;
        let parent = lookup.observe(&root, &EntryName::parse("parent")?).await?;
        let name = EntryName::parse("file")?;
        let entry = lookup.observe(&parent, &name).await?;

        std::fs::rename(base.join("parent"), base.join("moved"))?;
        std::os::unix::fs::symlink("other", base.join("parent"))?;
        let slot = EntrySlot {
            parent: &parent,
            name: &name,
            entry: Some(&entry),
        };
        let request =
            RemoveRequest::new(StoragePath::parse("parent/file")?, RemoveKind::NonDirectory)?;
        let error = lookup
            .execute_remove(slot, request)
            .await
            .err()
            .ok_or("replaced parent passed verification")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::Conflict));
        assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unchanged);
        assert_eq!(std::fs::read(base.join("moved/file"))?, b"observed");
        assert_eq!(std::fs::read(base.join("other/file"))?, b"referent");
        Ok(())
    }

    #[tokio::test]
    async fn a_binding_from_a_retired_lookup_is_stale() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("file"), b"unchanged")?;
        let root = Arc::new(Dir::open_ambient_dir(
            directory.path(),
            cap_std::ambient_authority(),
        )?);
        let op = Operation::Remove;
        let mut retired = LocalLookup::new(root.clone(), Arc::default(), op);
        let parent = retired.root().await?;
        let name = EntryName::parse("file")?;
        let file = retired.observe(&parent, &name).await?;
        drop(retired);
        let current = LocalLookup::new(root, Arc::default(), op);
        let slot = EntrySlot {
            parent: &parent,
            name: &name,
            entry: Some(&file),
        };
        let request = RemoveRequest::new(file.entry().path().clone(), RemoveKind::NonDirectory)?;
        let error = current
            .execute_remove(slot, request)
            .await
            .err()
            .ok_or("retired binding accepted")?;
        assert_eq!(error.failure(), Failure::Backend(BackendFailure::Stale));
        assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unchanged);
        assert_eq!(std::fs::read(directory.path().join("file"))?, b"unchanged");
        Ok(())
    }

    #[tokio::test]
    async fn single_mkdir_rejects_a_slot_occupied_after_lookup()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let root = Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority())?;
        let mut lookup =
            LocalLookup::new(Arc::new(root), Arc::default(), Operation::CreateDirectory);
        let parent = lookup.root().await?;
        let name = EntryName::parse("occupied")?;
        let address = StoragePath::parse("occupied")?;
        let original = lookup
            .observe(&parent, &name)
            .await
            .err()
            .ok_or("expected an empty slot")?;
        assert_eq!(original.failure(), Failure::Rejected(Rejection::NotFound));

        let target = directory.path().join("occupied");
        std::fs::write(&target, b"other writer")?;
        let error = lookup
            .execute_create_directory(
                EntrySlot {
                    parent: &parent,
                    name: &name,
                    entry: None,
                },
                CreateDirectoryRequest::new(address)?,
            )
            .await
            .err()
            .ok_or("mkdir accepted an occupied slot")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::AlreadyExists));
        assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unchanged);
        assert_eq!(std::fs::read(target)?, b"other writer");
        Ok(())
    }
}
