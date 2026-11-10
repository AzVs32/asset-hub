//! Exact metadata lookup through pinned Windows directory handles.
use super::{fs, native};
use ah_vfs::application::port::LookupContext;
use ah_vfs_storage::{
    BackendFailure, CreateDirectoryRequest, Entry, Failure, IntermediateSymlinks, LookupStart,
    MissingName, Operation, PathComponent, PathLookup, Rejection, StorageError, StoragePath,
    SymlinkAction,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions, OpenOptionsExt};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
};

fn rejected(op: Operation, reason: Rejection) -> StorageError {
    StorageError::without_effects(op, Failure::Rejected(reason), None)
}
fn stale(op: Operation) -> StorageError {
    StorageError::without_effects(op, Failure::Backend(BackendFailure::Stale), None)
}

pub(super) fn stat(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<Entry, StorageError> {
    Ok(select(root, lookup, context, Operation::Stat)?.0)
}

pub(super) fn open_reader(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<File, StorageError> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{FILE_GENERIC_READ, FILE_SHARE_DELETE, ReOpenFile},
    };
    let op = Operation::Read;
    let (_, selected, _) = select(root, lookup, context, op)?;
    let selected = selected.ok_or_else(|| rejected(op, Rejection::IsDirectory))?;
    // SAFETY: the selected handle stays live for the call. ReOpenFile returns
    // a new owned handle to that object, regardless of its namespace address.
    let handle = unsafe {
        ReOpenFile(
            selected.as_raw_handle(),
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_OPEN_REPARSE_POINT,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(fs::error(op, std::io::Error::last_os_error()));
    }
    // SAFETY: the successful call transferred ownership of this distinct handle.
    let file = File::from_std(unsafe { std::fs::File::from_raw_handle(handle) });
    let metadata = file.metadata().map_err(|cause| fs::error(op, cause))?;
    fs::regular_file(op, &metadata)?;
    Ok(file)
}

fn select(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
    op: Operation,
) -> Result<(Entry, Option<File>, Vec<(StoragePath, Dir)>), StorageError> {
    walk(root, lookup, context, op, &mut false)
}

pub(super) fn create_dir(
    root: &Dir,
    request: &CreateDirectoryRequest,
    context: LookupContext,
) -> Result<Entry, StorageError> {
    let op = Operation::CreateDirectory;
    let mut created = false;
    walk(root, request.lookup(), context, op, &mut created)
        .map(|(entry, _, _)| entry)
        .map_err(|error| {
            if created {
                StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
            } else {
                error
            }
        })
}

fn walk(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
    op: Operation,
    created: &mut bool,
) -> Result<(Entry, Option<File>, Vec<(StoragePath, Dir)>), StorageError> {
    let root_dir = root.try_clone().map_err(|cause| fs::error(op, cause))?;
    let mut stack = vec![(StoragePath::root(), root_dir)];
    if lookup.path().start() == LookupStart::BaseDirectory {
        for name in context.working_directory.entry_names() {
            let (parent, dir) = stack.last().ok_or_else(|| stale(op))?;
            let selected = open(op, dir, name.as_str()).map_err(|error| match error.failure() {
                Failure::Rejected(Rejection::NotFound | Rejection::NotDirectory) => stale(op),
                _ => error,
            })?;
            let (dir, metadata) = selected;
            if !metadata.is_dir() || fs::reparse(&metadata) {
                return Err(stale(op));
            }
            let dir = Dir::from_std_file(dir.into_std());
            let path = parent.join(&name).map_err(|_| stale(op))?;
            stack.push((path, dir));
        }
    }
    let mut final_entry = None;
    let components = lookup.path().components();
    for (index, component) in components.iter().enumerate() {
        match component {
            PathComponent::CurrentDirectory => {}
            PathComponent::ParentDirectory if stack.len() > 1 => {
                stack.pop();
            }
            PathComponent::ParentDirectory => {}
            PathComponent::Name(name) => {
                let (parent, dir) = stack.last().ok_or_else(|| stale(op))?;
                let path = parent
                    .join(name)
                    .map_err(|_| rejected(op, Rejection::PathTooLong))?;
                let last = index + 1 == components.len();
                let (file, metadata) = match open(op, dir, name.as_str()) {
                    Err(error)
                        if op == Operation::CreateDirectory
                            && error.failure() == Failure::Rejected(Rejection::NotFound) =>
                    {
                        let missing = if last {
                            MissingName::OperandFinal
                        } else {
                            MissingName::OperandIntermediate
                        };
                        lookup
                            .check_missing_name(&path, missing)
                            .map_err(|reason| rejected(op, reason))?;
                        create_child(dir, name.as_str(), created)?
                    }
                    result => result?,
                };
                if fs::reparse(&metadata) {
                    if !last || lookup.final_symlink() == SymlinkAction::FollowWithinInstance {
                        return Err(rejected(
                            op,
                            if !last
                                && metadata.file_type().is_symlink()
                                && context.intermediate_symlinks == IntermediateSymlinks::Reject
                            {
                                Rejection::SymlinkNotAllowed
                            } else {
                                // Native reparse data has no supported lossless traversal here.
                                Rejection::Unsupported
                            },
                        ));
                    }
                }
                if last {
                    final_entry = Some((fs::entry(op, path, metadata)?, Some(file)));
                } else {
                    if !metadata.is_dir() {
                        return Err(rejected(op, Rejection::NotDirectory));
                    }
                    stack.push((path, Dir::from_std_file(file.into_std())));
                }
            }
        }
    }
    let selected = match final_entry {
        Some(entry) => entry,
        None => {
            let (path, dir) = stack.last().ok_or_else(|| stale(op))?;
            let metadata = dir.dir_metadata().map_err(|cause| fs::error(op, cause))?;
            (fs::entry(op, path.clone(), metadata)?, None)
        }
    };
    if !*created || lookup.final_symlink() != SymlinkAction::Preserve {
        lookup
            .check_existing_target(selected.0.path(), selected.0.entry_type())
            .map_err(|reason| rejected(op, reason))?;
    }
    Ok((selected.0, selected.1, stack))
}

fn create_child(
    dir: &Dir,
    name: &str,
    created: &mut bool,
) -> Result<(File, Metadata), StorageError> {
    use std::io::ErrorKind;
    let op = Operation::CreateDirectory;
    match dir.create_dir(name) {
        Ok(()) => {
            *created = true;
            let (file, metadata) = open(op, dir, name)?;
            if !metadata.is_dir() || fs::reparse(&metadata) {
                return Err(stale(op));
            }
            Ok((file, metadata))
        }
        Err(cause) if cause.kind() == ErrorKind::AlreadyExists => {
            open(op, dir, name).map_err(|error| {
                if error.failure() == Failure::Rejected(Rejection::NotFound) {
                    rejected(op, Rejection::AlreadyExists)
                } else {
                    error
                }
            })
        }
        Err(cause)
            if matches!(
                cause.kind(),
                ErrorKind::NotFound
                    | ErrorKind::NotADirectory
                    | ErrorKind::PermissionDenied
                    | ErrorKind::InvalidInput
                    | ErrorKind::StorageFull
                    | ErrorKind::QuotaExceeded
                    | ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            Err(fs::error(op, cause))
        }
        Err(cause) => Err(fs::dispatched_error(op, cause)),
    }
}

fn open(op: Operation, dir: &Dir, name: &str) -> Result<(File, Metadata), StorageError> {
    fs::validate_name(op, name)?;
    let mut options = OpenOptions::new();
    let share = if op == Operation::Write {
        FILE_SHARE_READ
    } else {
        FILE_SHARE_READ | FILE_SHARE_WRITE
    };
    options
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(share)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let file = dir
        .open_with(name, &options)
        .map_err(|cause| fs::error(op, cause))?;
    if native::actual_name(op, &file)? != name {
        return Err(rejected(op, Rejection::NotFound));
    }
    let metadata = file.metadata().map_err(|cause| fs::error(op, cause))?;
    Ok((file, metadata))
}

pub(super) struct WriteDestination {
    pub parent: Dir,
    pub path: StoragePath,
    // Ancestors were opened without delete sharing and remain bound for publication.
    pub ancestors: Vec<(StoragePath, Dir)>,
}

pub(super) fn write_destination(
    root: &Dir,
    request: &ah_vfs_storage::WriteRequest,
    context: LookupContext,
) -> Result<WriteDestination, StorageError> {
    use ah_vfs_storage::{EntryType, IntermediateSymlinks, StatRequest};
    let op = Operation::Write;
    let Some((parent, name)) = request.path().split_final_name() else {
        select(root, request.lookup(), context, op)?;
        return Err(rejected(op, Rejection::NotDirectory));
    };
    let action = match context.intermediate_symlinks {
        IntermediateSymlinks::FollowWithinInstance => SymlinkAction::FollowWithinInstance,
        IntermediateSymlinks::Reject => SymlinkAction::Preserve,
    };
    let (entry, file, mut ancestors) =
        select(root, StatRequest::new(parent, action).lookup(), context, op)?;
    match entry.entry_type() {
        EntryType::Directory => {}
        EntryType::Symlink => return Err(rejected(op, Rejection::SymlinkNotAllowed)),
        _ => return Err(rejected(op, Rejection::NotDirectory)),
    }
    let parent = match file {
        Some(file) => Dir::from_std_file(file.into_std()),
        None => ancestors.pop().ok_or_else(|| stale(op))?.1,
    };
    let path = entry
        .path()
        .join(name)
        .map_err(|_| rejected(op, Rejection::PathTooLong))?;
    match open(op, &parent, name.as_str()) {
        Ok((_, metadata)) => {
            let entry = fs::entry(op, path.clone(), metadata)?;
            request
                .lookup()
                .check_existing_target(&path, entry.entry_type())
                .map_err(|reason| rejected(op, reason))?;
        }
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => {
            request
                .check_new_destination(&path)
                .map_err(|reason| rejected(op, reason))?;
        }
        Err(error) => return Err(error),
    }
    Ok(WriteDestination {
        parent,
        path,
        ancestors,
    })
}

pub(super) fn listing_directory(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<(StoragePath, Dir), StorageError> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{FILE_GENERIC_READ, FILE_SHARE_DELETE, ReOpenFile},
    };
    let op = Operation::List;
    let (entry, file, stack) = select(root, lookup, context, op)?;
    if entry.entry_type() != ah_vfs_storage::EntryType::Directory {
        return Err(rejected(op, Rejection::NotDirectory));
    }
    let handle = match &file {
        Some(file) => file.as_raw_handle(),
        None => stack.last().ok_or_else(list_stale)?.1.as_raw_handle(),
    };
    // SAFETY: the original handle stays live and the successful call transfers a new handle.
    let handle = unsafe {
        ReOpenFile(
            handle,
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(fs::error(op, std::io::Error::last_os_error()));
    }
    let dir = Dir::from_std_file(unsafe { std::fs::File::from_raw_handle(handle) });
    Ok((entry.path().clone(), dir))
}

pub(super) fn same_directory(left: &Dir, right: &Dir) -> Result<bool, StorageError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let identity = |dir: &Dir| {
        let mut data = std::mem::MaybeUninit::<FILE_ID_INFO>::uninit();
        // SAFETY: a live directory handle and writable output structure are supplied.
        if unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle(),
                FileIdInfo,
                data.as_mut_ptr().cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(fs::error(Operation::List, std::io::Error::last_os_error()));
        }
        let data = unsafe { data.assume_init() };
        Ok((data.VolumeSerialNumber, data.FileId.Identifier))
    };
    Ok(identity(left)? == identity(right)?)
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
