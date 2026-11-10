//! Handle-bound metadata and link lookup on Linux.
use super::fs;
use ah_vfs::application::port::LookupContext;
use ah_vfs_storage::{
    BackendFailure, CreateDirectoryRequest, Entry, EntryType, Failure, LinkTarget, LookupStart,
    MissingName, Operation, PathComponent, PathExpr, PathLookup, Rejection, StorageError,
    StoragePath, SymlinkAction, SymlinkTraversal,
};
use cap_std::fs::{Dir, File};
use rustix::fs::{Mode, OFlags, ResolveFlags, fstat, mkdirat, openat2, readlinkat};
use rustix::io::Errno;
use std::{collections::BTreeMap, sync::Arc};

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

pub(super) fn stat(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<Entry, StorageError> {
    Ok(Walk::select(root, lookup, context, Operation::Stat)?.entry)
}

pub(super) fn read_link(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<LinkTarget, StorageError> {
    let op = Operation::ReadLink;
    let selected = Walk::select(root, lookup, context, op)?;
    link_bytes(op, &selected.file)
}

pub(super) fn attribute_target(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
    op: Operation,
) -> Result<(Entry, Arc<File>), StorageError> {
    let selected = Walk::select(root, lookup, context, op)?;
    Ok((selected.entry, selected.file))
}

pub(super) fn descriptor_path(file: &File) -> String {
    use std::os::fd::AsRawFd;
    format!("/proc/self/fd/{}", file.as_raw_fd())
}

pub(super) fn open_reader(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<File, StorageError> {
    let op = Operation::Read;
    let selected = Walk::select(root, lookup, context, op)?;
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
        return Err(stale(op));
    }
    Ok(file)
}

pub(super) fn create_dir(
    root: &Dir,
    request: &CreateDirectoryRequest,
    context: LookupContext,
) -> Result<Entry, StorageError> {
    let op = Operation::CreateDirectory;
    let mut walk = Walk::new(root, request.lookup(), &context, op);
    let result = walk.resolve(&context).map(|selected| selected.entry);
    result.map_err(|error| {
        if walk.created {
            StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
        } else {
            error
        }
    })
}

struct Slot {
    parent: Selected,
    path: StoragePath,
    name: String,
}

pub(super) fn remove(
    root: &Dir,
    request: &ah_vfs_storage::RemoveRequest,
    context: LookupContext,
) -> Result<(), StorageError> {
    use ah_vfs_storage::RemoveKind;
    use rustix::fs::{AtFlags, unlinkat};
    let op = Operation::Remove;
    let lookup = request.lookup();
    let mut walk = Walk::new(root, lookup, &context, op);
    let base = walk.start(
        &context,
        lookup.path().start() == LookupStart::BaseDirectory,
    )?;
    let slot = walk.mutation_slot(base)?;
    let selected = walk.child(&slot.parent, slot.path.clone(), &slot.name)?;
    lookup
        .check_existing_target(&slot.path, selected.entry.entry_type())
        .map_err(|reason| rejected(op, reason))?;
    walk.verify_bound()?;
    // The kernel checks the actual entry kind and emptiness in the unlink operation.
    let flags = match request.kind() {
        RemoveKind::NonDirectory => AtFlags::empty(),
        RemoveKind::EmptyDirectory => AtFlags::REMOVEDIR,
    };
    unlinkat(slot.parent.file.as_ref(), slot.name.as_str(), flags)
        .map_err(|cause| mutation_error(op, cause))
}

pub(super) fn rename(
    root: &Dir,
    request: &ah_vfs_storage::RenameRequest,
    context: LookupContext,
) -> Result<Entry, StorageError> {
    use ah_vfs_storage::{RenameDecision, RenameDestination, RenameMode};
    use rustix::fs::{RenameFlags, renameat_with};
    let op = Operation::Rename;
    let (source_lookup, destination_lookup) = request.lookups();
    let mut walk = Walk::new(root, source_lookup, &context, op);
    let relative = |lookup: PathLookup<'_>| lookup.path().start() == LookupStart::BaseDirectory;
    let base = walk.start(
        &context,
        relative(source_lookup) || relative(destination_lookup),
    )?;
    let source = walk.mutation_slot(base.clone())?;
    let selected = walk.child(&source.parent, source.path.clone(), &source.name)?;
    let kind = selected.entry.entry_type();
    source_lookup
        .check_existing_target(&source.path, kind)
        .map_err(|reason| rejected(op, reason))?;

    walk.lookup = destination_lookup;
    let destination = walk.mutation_slot(base)?;
    let observed = if source.name == destination.name
        && identity(op, &source.parent.file)? == identity(op, &destination.parent.file)?
    {
        RenameDestination::SameSlot
    } else {
        match walk.child(
            &destination.parent,
            destination.path.clone(),
            &destination.name,
        ) {
            Ok(target)
                if target.entry.entry_type() == EntryType::Directory
                    && request.mode() == RenameMode::Replace
                    && kind == EntryType::Directory
                    && !empty_directory(&target.file)? =>
            {
                RenameDestination::NonEmptyDirectory
            }
            Ok(target) => RenameDestination::Occupied(target.entry.entry_type()),
            Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => {
                RenameDestination::Missing
            }
            Err(error) => return Err(error),
        }
    };
    let decision = request
        .decide_destination(&destination.path, kind, observed)
        .map_err(|reason| rejected(op, reason))?;
    walk.verify_bound()?;
    let result =
        Entry::new(destination.path.clone(), kind, selected.entry.modified()).map_err(|cause| {
            StorageError::without_effects(
                op,
                Failure::Contract(ah_vfs_storage::ContractViolation::InvalidEntry),
                Some(Box::new(cause)),
            )
        })?;
    if decision == RenameDecision::Unchanged {
        return Ok(result);
    }
    if observed != RenameDestination::Missing {
        // renameat2 has no compare-and-replace condition for an occupied slot.
        return Err(rejected(op, Rejection::Unsupported));
    }
    let source_name =
        if source_lookup.requires_directory() || destination_lookup.requires_directory() {
            format!("{}/", source.name)
        } else {
            source.name
        };
    renameat_with(
        source.parent.file.as_ref(),
        source_name.as_str(),
        destination.parent.file.as_ref(),
        destination.name.as_str(),
        RenameFlags::NOREPLACE,
    )
    .map_err(|cause| {
        if cause == Errno::EXIST && request.mode() == RenameMode::Replace {
            rejected(op, Rejection::Conflict)
        } else {
            mutation_error(op, cause)
        }
    })?;
    // Check that the published entry is the source inode selected by this walk.
    let published = walk
        .child(&destination.parent, destination.path, &destination.name)
        .and_then(|target| {
            if identity(op, &selected.file)? == identity(op, &target.file)? {
                Ok(())
            } else {
                Err(stale(op))
            }
        });
    published.map_err(|error| {
        StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
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
        Errno::INVAL if op == Operation::Rename => rejected(op, Rejection::Conflict),
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

struct Walk<'a, 'r> {
    root: &'a Dir,
    lookup: PathLookup<'r>,
    links: SymlinkTraversal,
    op: Operation,
    created: bool,
    // Keep the handles used to resolve the operand, even when navigation
    // leaves a directory or follows a link. Revalidate before returning.
    bound: BTreeMap<StoragePath, Arc<File>>,
}

impl<'a, 'r> Walk<'a, 'r> {
    fn select(
        root: &'a Dir,
        lookup: PathLookup<'r>,
        context: LookupContext,
        op: Operation,
    ) -> Result<Selected, StorageError> {
        Self::new(root, lookup, &context, op).resolve(&context)
    }

    fn new(root: &'a Dir, lookup: PathLookup<'r>, context: &LookupContext, op: Operation) -> Self {
        Self {
            root,
            lookup,
            links: SymlinkTraversal::new(context.intermediate_symlinks),
            op,
            created: false,
            bound: BTreeMap::new(),
        }
    }

    fn resolve(&mut self, context: &LookupContext) -> Result<Selected, StorageError> {
        let op = self.op;
        let lookup = self.lookup;
        let mut directories =
            self.start(context, lookup.path().start() == LookupStart::BaseDirectory)?;
        let walk = self;
        let selected = walk.segment(lookup.path(), &mut directories, &[], false)?;
        // Single can create only the final component. Its successful creation
        // has already satisfied the absent-target policy.
        if !walk.created || lookup.final_symlink() != SymlinkAction::Preserve {
            lookup
                .check_existing_target(selected.entry.path(), selected.entry.entry_type())
                .map_err(|reason| rejected(op, reason))?;
        }
        walk.verify_bound()?;
        Ok(selected)
    }

    fn start(
        &mut self,
        context: &LookupContext,
        relative: bool,
    ) -> Result<Vec<Selected>, StorageError> {
        let op = self.op;
        let mut directories =
            vec![self.selected(StoragePath::root(), self.open_file(&StoragePath::root())?)?];
        if relative {
            let mut base = StoragePath::root();
            for name in context.working_directory.entry_names() {
                base = base.join(&name).map_err(|_| stale(op))?;
                let parent = directories.last().ok_or_else(|| stale(op))?;
                let selected =
                    self.child(parent, base.clone(), name.as_str())
                        .map_err(|error| match error.failure() {
                            Failure::Rejected(
                                Rejection::NotFound
                                | Rejection::NotDirectory
                                | Rejection::SymlinkNotAllowed,
                            ) => stale(op),
                            _ => error,
                        })?;
                if selected.entry.entry_type() != EntryType::Directory {
                    return Err(stale(op));
                }
                directories.push(selected);
            }
        }
        Ok(directories)
    }

    fn mutation_slot(&mut self, mut directories: Vec<Selected>) -> Result<Slot, StorageError> {
        let (operand, name) = self
            .lookup
            .path()
            .split_final_name()
            .ok_or_else(|| rejected(self.op, Rejection::RootProtected))?;
        let parent = self.segment(
            &operand,
            &mut directories,
            &[PathComponent::Name(name.clone())],
            false,
        )?;
        let path = parent
            .entry
            .path()
            .join(name)
            .map_err(|_| rejected(self.op, Rejection::PathTooLong))?;
        Ok(Slot {
            parent,
            path,
            name: name.as_str().to_owned(),
        })
    }

    fn open_file(&self, path: &StoragePath) -> Result<File, StorageError> {
        let name = if path.is_root() { "." } else { path.as_str() };
        openat2(
            self.root,
            name,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map(|fd| File::from_std(fd.into()))
        .map_err(|cause| match cause {
            Errno::NOSYS => rejected(self.op, Rejection::Unsupported),
            Errno::AGAIN | Errno::LOOP => stale(self.op),
            _ => fs::error(self.op, cause.into()),
        })
    }

    fn child(
        &mut self,
        parent: &Selected,
        path: StoragePath,
        name: &str,
    ) -> Result<Selected, StorageError> {
        let fd = openat2(
            parent.file.as_ref(),
            name,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|cause| fs::error(self.op, cause.into()))?;
        self.selected(path, File::from_std(fd.into()))
    }

    fn create_child(
        &mut self,
        parent: &Selected,
        path: StoragePath,
        name: &str,
        missing: MissingName,
    ) -> Result<Selected, StorageError> {
        self.lookup
            .check_missing_name(&path, missing)
            .map_err(|reason| rejected(self.op, reason))?;
        self.verify_bound()?;
        let mode = Mode::RUSR
            | Mode::WUSR
            | Mode::XUSR
            | Mode::RGRP
            | Mode::WGRP
            | Mode::XGRP
            | Mode::ROTH
            | Mode::WOTH
            | Mode::XOTH;
        match mkdirat(parent.file.as_ref(), name, mode) {
            Ok(()) => {
                self.created = true;
                let selected = self.child(parent, path, name)?;
                if selected.entry.entry_type() != EntryType::Directory {
                    return Err(stale(self.op));
                }
                Ok(selected)
            }
            Err(Errno::EXIST) => self.child(parent, path, name),
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
            ) => Err(fs::error(self.op, cause.into())),
            Err(cause) => Err(fs::dispatched_error(self.op, cause.into())),
        }
    }

    fn selected(&mut self, path: StoragePath, file: File) -> Result<Selected, StorageError> {
        let file = Arc::new(file);
        let metadata = file.metadata().map_err(|cause| fs::error(self.op, cause))?;
        let entry = fs::entry(self.op, path, metadata)?;
        if !entry.path().is_root() {
            match self.bound.entry(entry.path().clone()) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(file.clone());
                }
                std::collections::btree_map::Entry::Occupied(slot)
                    if identity(self.op, slot.get())? != identity(self.op, &file)? =>
                {
                    return Err(stale(self.op));
                }
                _ => {}
            }
        }
        Ok(Selected { entry, file })
    }

    fn verify_bound(&self) -> Result<(), StorageError> {
        for (path, bound) in &self.bound {
            let current = self
                .open_file(path)
                .map_err(|error| match error.failure() {
                    Failure::Rejected(Rejection::NotFound | Rejection::NotDirectory) => {
                        stale(self.op)
                    }
                    _ => error,
                })?;
            if identity(self.op, bound)? != identity(self.op, &current)? {
                return Err(stale(self.op));
            }
        }
        Ok(())
    }

    fn segment(
        &mut self,
        expression: &PathExpr,
        directories: &mut Vec<Selected>,
        suffix: &[PathComponent],
        in_link: bool,
    ) -> Result<Selected, StorageError> {
        if expression.start() == LookupStart::InstanceRoot {
            directories.truncate(1);
        }
        let mut terminal = None;
        for (index, component) in expression.components().iter().enumerate() {
            let parent = directories.last().ok_or_else(|| stale(self.op))?;
            match component {
                PathComponent::CurrentDirectory => {}
                PathComponent::ParentDirectory if directories.len() > 1 => {
                    directories.pop();
                }
                PathComponent::ParentDirectory => {}
                PathComponent::Name(name) => {
                    let path = parent
                        .entry
                        .path()
                        .join(name)
                        .map_err(|_| rejected(self.op, Rejection::PathTooLong))?;
                    let remaining = &expression.components()[index + 1..];
                    let mut selected = match self.child(parent, path.clone(), name.as_str()) {
                        Err(error)
                            if self.op == Operation::CreateDirectory
                                && error.failure() == Failure::Rejected(Rejection::NotFound) =>
                        {
                            let missing = if in_link {
                                MissingName::LinkTarget
                            } else if remaining.is_empty() {
                                MissingName::OperandFinal
                            } else {
                                MissingName::OperandIntermediate
                            };
                            self.create_child(parent, path, name.as_str(), missing)?
                        }
                        result => result?,
                    };
                    if selected.entry.entry_type() == EntryType::Symlink {
                        let pending: Vec<_> = remaining.iter().chain(suffix).cloned().collect();
                        if self
                            .links
                            .encounter(self.lookup, &pending)
                            .map_err(|reason| rejected(self.op, reason))?
                            == SymlinkAction::FollowWithinInstance
                        {
                            let target = link_bytes(self.op, &selected.file)?
                                .prepare_lookup(parent.entry.path(), &pending)
                                .map_err(|reason| failure(self.op, reason))?;
                            selected = self.segment(&target, directories, &pending, true)?;
                            if selected.entry.entry_type() == EntryType::Directory {
                                // The expanded walk already left its directory on the stack.
                                continue;
                            }
                        }
                    }
                    if selected.entry.entry_type() == EntryType::Directory {
                        directories.push(selected);
                    } else if remaining.is_empty() && suffix.is_empty() {
                        terminal = Some(selected);
                    } else {
                        return Err(rejected(self.op, Rejection::NotDirectory));
                    }
                }
            }
        }
        let selected = match terminal {
            Some(value) => value,
            None => {
                let directory = directories.last().ok_or_else(|| stale(self.op))?;
                Selected {
                    entry: directory.entry.clone(),
                    file: directory.file.clone(),
                }
            }
        };
        expression
            .check_directory_requirement(selected.entry.entry_type() == EntryType::Directory)
            .map_err(|reason| rejected(self.op, reason))?;
        Ok(selected)
    }
}

fn identity(op: Operation, file: &File) -> Result<impl Eq, StorageError> {
    fstat(file)
        .map(|stat| (stat.st_dev, stat.st_ino))
        .map_err(|cause| fs::error(op, cause.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ah_vfs_storage::StatRequest;

    #[test]
    fn cross_device_rename_leaves_both_slots_unchanged() -> Result<(), Box<dyn std::error::Error>> {
        use ah_vfs_storage::{MutationOutcome, RenameMode, RenameRequest};
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
        let request = RenameRequest::new(
            PathExpr::parse(from.to_str().ok_or("source")?)?,
            PathExpr::parse(to.to_str().ok_or("destination")?)?,
            RenameMode::NoReplace,
        )?;
        let error = rename(&root, &request, LookupContext::default())
            .err()
            .ok_or("cross-device rename succeeded")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::CrossDevice));
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
        assert_eq!(std::fs::read(from)?, b"original");
        assert!(!to.exists());
        Ok(())
    }

    #[test]
    fn replacing_a_bound_link_cannot_supply_new_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::os::unix::fs::symlink("before", directory.path().join("link"))?;
        let root = Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority())?;
        let request = StatRequest::new(PathExpr::parse("/link")?, SymlinkAction::Preserve);
        let mut walk = Walk {
            root: &root,
            lookup: request.lookup(),
            links: SymlinkTraversal::default(),
            op: Operation::Stat,
            created: false,
            bound: BTreeMap::new(),
        };
        let root_entry =
            walk.selected(StoragePath::root(), walk.open_file(&StoragePath::root())?)?;
        let link = walk.child(&root_entry, StoragePath::parse("link")?, "link")?;

        std::fs::rename(directory.path().join("link"), directory.path().join("old"))?;
        std::os::unix::fs::symlink("after", directory.path().join("link"))?;

        assert_eq!(
            link_bytes(Operation::ReadLink, &link.file)?.as_bytes(),
            b"before"
        );
        assert!(matches!(walk.verify_bound(), Err(error) if
            error.failure() == Failure::Backend(BackendFailure::Stale)));
        Ok(())
    }
}

pub(super) fn write_destination(
    root: &Dir,
    request: &ah_vfs_storage::WriteRequest,
    context: LookupContext,
) -> Result<(Dir, StoragePath), StorageError> {
    use ah_vfs_storage::{IntermediateSymlinks, StatRequest};
    let op = Operation::Write;
    let Some((parent, name)) = request.path().split_final_name() else {
        Walk::select(root, request.lookup(), context, op)?;
        return Err(rejected(op, Rejection::NotDirectory));
    };
    let action = match context.intermediate_symlinks {
        IntermediateSymlinks::FollowWithinInstance => SymlinkAction::FollowWithinInstance,
        IntermediateSymlinks::Reject => SymlinkAction::Preserve,
    };
    let parent = Walk::select(root, StatRequest::new(parent, action).lookup(), context, op)?;
    match parent.entry.entry_type() {
        EntryType::Directory => {}
        EntryType::Symlink => return Err(rejected(op, Rejection::SymlinkNotAllowed)),
        _ => return Err(rejected(op, Rejection::NotDirectory)),
    }
    let path = parent
        .entry
        .path()
        .join(name)
        .map_err(|_| rejected(op, Rejection::PathTooLong))?;
    let dir = Dir::from_std_file(
        parent
            .file
            .try_clone()
            .map_err(|cause| fs::error(op, cause))?
            .into_std(),
    );
    match dir.symlink_metadata(name.as_str()) {
        Ok(metadata) => {
            let entry = fs::entry(op, path.clone(), metadata)?;
            request
                .lookup()
                .check_existing_target(&path, entry.entry_type())
                .map_err(|reason| rejected(op, reason))?;
        }
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
            request
                .check_new_destination(&path)
                .map_err(|reason| rejected(op, reason))?;
        }
        Err(cause) => return Err(fs::error(op, cause)),
    }
    Ok((dir, path))
}

pub(super) fn listing_directory(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<(StoragePath, Dir), StorageError> {
    let op = Operation::List;
    let selected = Walk::select(root, lookup, context, op)?;
    if selected.entry.entry_type() != EntryType::Directory {
        return Err(rejected(op, Rejection::NotDirectory));
    }
    let dir = selected
        .file
        .try_clone()
        .map_err(|cause| fs::error(op, cause))?;
    Ok((
        selected.entry.path().clone(),
        Dir::from_std_file(dir.into_std()),
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

pub(super) fn reopen_listing_directory(
    root: &Dir,
    selection: &ah_vfs_storage::DirectorySelection,
) -> Result<Dir, StorageError> {
    use ah_vfs_storage::{IntermediateSymlinks, StatRequest};
    let context = LookupContext {
        working_directory: StoragePath::root(),
        intermediate_symlinks: IntermediateSymlinks::Reject,
    };
    let stat = StatRequest::new(
        selection.address().to_rooted_expr(),
        SymlinkAction::Preserve,
    );
    listing_directory(root, stat.lookup(), context)
        .map(|(_, dir)| dir)
        .map_err(|error| {
            if matches!(
                error.failure(),
                Failure::Rejected(
                    Rejection::NotFound
                        | Rejection::NotDirectory
                        | Rejection::SymlinkNotAllowed
                        | Rejection::Unsupported
                )
            ) {
                list_stale()
            } else {
                error
            }
        })
}
