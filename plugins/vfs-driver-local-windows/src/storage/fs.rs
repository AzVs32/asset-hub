//! Windows names, reparse-point policy and directory-relative filesystem I/O.
use super::{TEMP_PREFIX, native};
use ah_vfs::{
    application::dto::{StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageEntry, StorageEntryKind, StorageMoveEntryRequest},
    domain::{StorageError, StorageErrorKind, StoragePath},
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, MetadataExt, OpenOptions, OpenOptionsExt};
use std::io;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, SYNCHRONIZE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE,
};

pub(super) fn error(error: io::Error) -> StorageError {
    StorageError::with_source(crate::error::io_error_kind(&error), error)
}
pub(super) fn private_name(name: &str) -> bool { name.to_ascii_lowercase().starts_with(TEMP_PREFIX) }
pub(super) fn validate_name(name: &str) -> Result<(), StorageError> {
    if private_name(name) { return Err(StorageError::new(StorageErrorKind::PermissionDenied)); }
    if name.is_empty() || name.ends_with(['.', ' ']) || name.chars().any(|ch| ch <= '\u{1f}' || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')) {
        return Err(StorageError::new(StorageErrorKind::InvalidPath));
    }
    let stem = name.split('.').next().unwrap_or("").trim_end_matches(' ').to_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$")
        || ["COM", "LPT"].iter().any(|prefix| stem.strip_prefix(*prefix).is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        }));
    if reserved { return Err(StorageError::new(StorageErrorKind::InvalidPath)); }
    Ok(())
}
fn validate(path: &StoragePath) -> Result<(), StorageError> {
    for name in path.segments() { validate_name(name)?; }
    Ok(())
}
pub(super) fn reparse(metadata: &Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
pub(super) fn checked_metadata(parent: &Dir, name: &str) -> Result<Metadata, StorageError> {
    validate_name(name)?;
    native::open_public(parent, name, FILE_READ_ATTRIBUTES)?.metadata().map_err(error)
}
pub(super) fn dir(root: &Dir, path: &StoragePath, create: bool) -> Result<Dir, StorageError> {
    validate(path)?;
    let mut current = root.try_clone().map_err(error)?;
    for name in path.segments() {
        if create {
            match current.create_dir(name) {
                Ok(()) => {},
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {},
                Err(cause) => return Err(error(cause)),
            }
        }
        // Keep directory handles unshareable for deletion. Traversal opens the
        // reparse point itself and rejects it, including junctions and mount points.
        let mut options = OpenOptions::new();
        options.read(true).access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .follow(FollowSymlinks::No);
        let file = current.open_with(name, &options).map_err(error)?;
        native::public_handle(&file)?;
        let metadata = file.metadata().map_err(error)?;
        if reparse(&metadata) { return Err(StorageError::new(StorageErrorKind::UnsupportedOperation)); }
        if !metadata.is_dir() { return Err(StorageError::new(StorageErrorKind::NotDir)); }
        current = Dir::from_std_file(file.into_std());
    }
    Ok(current)
}
pub(super) fn parent(root: &Dir, path: &StoragePath) -> Result<(Dir, String), StorageError> {
    validate(path)?;
    let parent = path.parent().ok_or_else(|| StorageError::new(StorageErrorKind::RootProtected))?;
    let name = path.name().ok_or_else(|| StorageError::new(StorageErrorKind::RootProtected))?.to_owned();
    Ok((dir(root, &parent, false)?, name))
}
pub(super) fn entry(path: StoragePath, metadata: Metadata) -> StorageEntry {
    let kind = if metadata.file_type().is_symlink() { StorageEntryKind::Symlink }
        else if reparse(&metadata) { StorageEntryKind::Other }
        else if metadata.is_file() { StorageEntryKind::File }
        else if metadata.is_dir() { StorageEntryKind::Dir }
        else { StorageEntryKind::Other };
    StorageEntry { path, kind, size: (kind == StorageEntryKind::File).then(|| metadata.len()),
        modified: metadata.modified().ok().map(|time| time.into_std()) }
}
pub(super) fn stat_entry(root: &Dir, path: StoragePath) -> Result<StorageEntry, StorageError> {
    let metadata = if path.is_root() { root.dir_metadata().map_err(error)? } else {
        let (parent, name) = parent(root, &path)?;
        checked_metadata(&parent, &name)?
    };
    Ok(entry(path, metadata))
}
pub(super) fn regular_file(metadata: &Metadata) -> Result<(), StorageError> {
    if reparse(metadata) { return Err(StorageError::new(StorageErrorKind::UnsupportedOperation)); }
    if metadata.is_dir() { return Err(StorageError::new(StorageErrorKind::IsDir)); }
    if !metadata.is_file() { return Err(StorageError::new(StorageErrorKind::UnsupportedOperation)); }
    Ok(())
}
pub(super) fn open_file(root: &Dir, path: &StoragePath) -> Result<File, StorageError> {
    if path.is_root() { return Err(StorageError::new(StorageErrorKind::IsDir)); }
    let (parent, name) = parent(root, path)?;
    regular_file(&checked_metadata(&parent, &name)?)?;
    let mut options = OpenOptions::new();
    options.read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).follow(FollowSymlinks::No);
    let file = parent.open_with(&name, &options).map_err(error)?;
    native::public_handle(&file)?;
    regular_file(&file.metadata().map_err(error)?)?;
    Ok(file)
}
pub(super) fn list_entries(root: &Dir, query: StorageDirQuery) -> Result<StorageDirPage, StorageError> {
    query.validate()?;
    if let Some(after) = &query.after { validate_name(after)?; }
    let dir = dir(root, &query.path, false)?;
    let mut names = vec![];
    for child in dir.entries().map_err(error)? {
        let name = child.map_err(error)?.file_name().into_string()
            .map_err(|_| StorageError::new(StorageErrorKind::InvalidNameEncoding))?;
        if private_name(&name) { continue; }
        validate_name(&name).map_err(|_| StorageError::new(StorageErrorKind::InvalidNameEncoding))?;
        query.path.join(&name)?;
        if query.after.as_ref().is_none_or(|after| name > *after) { names.push(name); }
    }
    names.sort();
    let more = names.len() > query.limit.get();
    names.truncate(query.limit.get());
    let next_after = more.then(|| names.last().cloned()).flatten();
    let entries = names.into_iter().map(|name| {
        Ok(entry(query.path.join(&name)?, checked_metadata(&dir, &name)?))
    }).collect::<Result<_, StorageError>>()?;
    Ok(StorageDirPage { entries, next_after })
}
pub(super) fn create_dir(root: &Dir, request: StorageCreateDirRequest) -> Result<StorageEntry, StorageError> {
    if request.path.is_root() && !request.recursive { return Err(StorageError::new(StorageErrorKind::AlreadyExists)); }
    if request.recursive {
        let dir = dir(root, &request.path, true)?;
        Ok(entry(request.path, dir.dir_metadata().map_err(error)?))
    } else {
        let (parent, name) = parent(root, &request.path)?;
        parent.create_dir(&name).map_err(error)?;
        Ok(entry(request.path, checked_metadata(&parent, &name)?))
    }
}
pub(super) fn remove_entry(root: &Dir, path: StoragePath) -> Result<(), StorageError> {
    let (parent, name) = parent(root, &path)?;
    let file = native::mutation_handle(&parent, &name)?;
    if reparse(&file.metadata().map_err(error)?) { return Err(StorageError::new(StorageErrorKind::UnsupportedOperation)); }
    native::delete(&file)
}
pub(super) fn move_entry(root: &Dir, request: StorageMoveEntryRequest) -> Result<StorageEntry, StorageError> {
    let (source_parent, source) = parent(root, &request.source)?;
    let (destination_parent, destination) = parent(root, &request.destination)?;
    let file = native::mutation_handle(&source_parent, &source)?;
    let metadata = file.metadata().map_err(error)?;
    if reparse(&metadata) { return Err(StorageError::new(StorageErrorKind::UnsupportedOperation)); }
    if request.source == request.destination {
        if !request.replace { return Err(StorageError::new(StorageErrorKind::AlreadyExists)); }
        return Ok(entry(request.destination, metadata));
    }
    native::rename(&file, &destination_parent, &destination, request.replace)?;
    Ok(entry(request.destination, metadata))
}
