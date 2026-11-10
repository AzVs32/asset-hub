//! Exact metadata lookup through pinned Windows directory handles.
use super::{fs, native};
use ah_vfs::application::port::LookupContext;
use ah_vfs_storage::{
    BackendFailure, Entry, Failure, IntermediateSymlinks, LookupStart, Operation, PathComponent,
    PathLookup, Rejection, StorageError, StoragePath, SymlinkAction,
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
    let (_, selected) = select(root, lookup, context, op)?;
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
    let file = File::from_std(unsafe {
        std::fs::File::from_raw_handle(handle)
    });
    let metadata = file.metadata().map_err(|cause| fs::error(op, cause))?;
    fs::regular_file(op, &metadata)?;
    Ok(file)
}

fn select(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
    op: Operation,
) -> Result<(Entry, Option<File>), StorageError> {
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
                let (file, metadata) = open(op, dir, name.as_str())?;
                let last = index + 1 == components.len();
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
    lookup
        .check_existing_target(selected.0.path(), selected.0.entry_type())
        .map_err(|reason| rejected(op, reason))?;
    Ok(selected)
}

fn open(op: Operation, dir: &Dir, name: &str) -> Result<(File, Metadata), StorageError> {
    fs::validate_name(op, name)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
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
