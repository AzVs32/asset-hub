//! Handle-bound metadata and link lookup on Linux.
use super::fs;
use ah_vfs::application::port::LookupContext;
use ah_vfs_storage::{
    BackendFailure, Entry, EntryType, Failure, LinkTarget, LookupStart, Operation, PathComponent,
    PathExpr, PathLookup, Rejection, StorageError, StoragePath, SymlinkAction, SymlinkTraversal,
};
use cap_std::fs::{Dir, File};
use rustix::fs::{Mode, OFlags, ResolveFlags, fstat, openat2, readlinkat};
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

pub(super) fn open_reader(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<File, StorageError> {
    use std::os::fd::AsRawFd;
    let op = Operation::Read;
    let selected = Walk::select(root, lookup, context, op)?;
    // The procfs descriptor refers to the pinned inode even after a rename.
    // Reopening the selected name would race a replacement with a different file.
    let descriptor = format!("/proc/self/fd/{}", selected.file.as_raw_fd());
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
        let mut walk = Self {
            root,
            lookup,
            links: SymlinkTraversal::new(context.intermediate_symlinks),
            op,
            bound: BTreeMap::new(),
        };
        let mut directories =
            vec![walk.selected(StoragePath::root(), walk.open_file(&StoragePath::root())?)?];
        if lookup.path().start() == LookupStart::BaseDirectory {
            let mut base = StoragePath::root();
            for name in context.working_directory.entry_names() {
                base = base.join(&name).map_err(|_| stale(op))?;
                let parent = directories.last().ok_or_else(|| stale(op))?;
                let selected =
                    walk.child(parent, base.clone(), name.as_str())
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
        let selected = walk.segment(lookup.path(), &mut directories, &[])?;
        lookup
            .check_existing_target(selected.entry.path(), selected.entry.entry_type())
            .map_err(|reason| rejected(op, reason))?;
        walk.verify_bound()?;
        Ok(selected)
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
        fs::validate(self.op, &path)?;
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
                    let mut selected = self.child(parent, path, name.as_str())?;
                    let remaining = &expression.components()[index + 1..];
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
                            selected = self.segment(&target, directories, &pending)?;
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
