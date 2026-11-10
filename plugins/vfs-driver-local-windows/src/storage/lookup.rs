//! Exact metadata lookup through pinned Windows directory handles.
use super::{fs, native};
use ah_vfs::application::port::LookupContext;
use ah_vfs_storage::{
    BackendFailure, Entry, Failure, IntermediateSymlinks, LookupStart, Operation, PathComponent,
    PathLookup, Rejection, StorageError, StoragePath, SymlinkAction,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, Metadata, OpenOptions, OpenOptionsExt};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
};

const OP: Operation = Operation::Stat;
fn rejected(reason: Rejection) -> StorageError {
    StorageError::without_effects(OP, Failure::Rejected(reason), None)
}
fn stale() -> StorageError {
    StorageError::without_effects(OP, Failure::Backend(BackendFailure::Stale), None)
}

pub(super) fn stat(
    root: &Dir,
    lookup: PathLookup<'_>,
    context: LookupContext,
) -> Result<Entry, StorageError> {
    let root_dir = root.try_clone().map_err(|cause| fs::error(OP, cause))?;
    let mut stack = vec![(StoragePath::root(), root_dir)];
    if lookup.path().start() == LookupStart::BaseDirectory {
        for name in context.working_directory.entry_names() {
            let (parent, dir) = stack.last().ok_or_else(stale)?;
            let selected = open(dir, name.as_str()).map_err(|error| match error.failure() {
                Failure::Rejected(Rejection::NotFound | Rejection::NotDirectory) => stale(),
                _ => error,
            })?;
            let (Some(dir), _) = selected else {
                return Err(stale());
            };
            let path = parent.join(&name).map_err(|_| stale())?;
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
                let (parent, dir) = stack.last().ok_or_else(stale)?;
                let path = parent
                    .join(name)
                    .map_err(|_| rejected(Rejection::PathTooLong))?;
                let (opened_directory, metadata) = open(dir, name.as_str())?;
                let last = index + 1 == components.len();
                if fs::reparse(&metadata) {
                    if !last || lookup.final_symlink() == SymlinkAction::FollowWithinInstance {
                        return Err(rejected(
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
                    final_entry = Some(fs::entry(OP, path, metadata)?);
                } else {
                    let dir = opened_directory.ok_or_else(|| rejected(Rejection::NotDirectory))?;
                    stack.push((path, dir));
                }
            }
        }
    }
    let selected = match final_entry {
        Some(entry) => entry,
        None => {
            let (path, dir) = stack.last().ok_or_else(stale)?;
            let metadata = dir.dir_metadata().map_err(|cause| fs::error(OP, cause))?;
            fs::entry(OP, path.clone(), metadata)?
        }
    };
    lookup
        .check_existing_target(selected.path(), selected.entry_type())
        .map_err(rejected)?;
    Ok(selected)
}

fn open(dir: &Dir, name: &str) -> Result<(Option<Dir>, Metadata), StorageError> {
    fs::validate_name(OP, name)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let file = dir
        .open_with(name, &options)
        .map_err(|cause| fs::error(OP, cause))?;
    if native::actual_name(OP, &file)? != name {
        return Err(rejected(Rejection::NotFound));
    }
    let metadata = file.metadata().map_err(|cause| fs::error(OP, cause))?;
    let opened_directory =
        (metadata.is_dir() && !fs::reparse(&metadata)).then(|| Dir::from_std_file(file.into_std()));
    Ok((opened_directory, metadata))
}
