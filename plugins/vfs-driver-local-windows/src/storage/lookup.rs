//! Exact, no-follow observations through Windows file and directory handles.
use super::{blocking, file_reader::LocalFileReader, file_writer::LocalFileWriter, fs, native};
use ah_vfs::application::port::{
    DirectoryBindings, EntrySlot, LookupScope, ObservedEntry, OperationLookup, StorageFileReader,
    StorageFileWriter, StorageFuture,
};
use ah_vfs_storage::{
    BackendFailure, CreateDirectoryRequest, DirectorySelection, Entry, EntryName, EntryType,
    Failure, LinkTarget, ListRequest, Operation, ReadRequest, Rejection, RemoveRequest,
    RenameDestination, RenameMode, RenameRequest, StatRequest, StorageError, StoragePath,
    WriteRequest,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions, OpenOptionsExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    os::windows::io::{AsRawHandle, FromRawHandle, RawHandle},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_ID_INFO,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo,
        GetFileInformationByHandleEx, ReOpenFile, SYNCHRONIZE,
    },
};

fn rejected(op: Operation, reason: Rejection) -> StorageError {
    StorageError::without_effects(op, Failure::Rejected(reason), None)
}
fn stale(op: Operation) -> StorageError {
    StorageError::without_effects(op, Failure::Backend(BackendFailure::Stale), None)
}

fn invalid(op: Operation) -> StorageError {
    StorageError::without_effects(op, Failure::InvalidBinding, None)
}

#[derive(Clone)]
struct Selected {
    entry: Entry,
    file: Option<Arc<File>>,
}

impl Selected {
    fn directory(&self, root: &Dir, op: Operation) -> Result<Dir, StorageError> {
        if self.entry.entry_type() != EntryType::Directory {
            return Err(rejected(op, Rejection::NotDirectory));
        }
        match &self.file {
            None => root.try_clone().map_err(|cause| fs::error(op, cause)),
            Some(file) => Ok(Dir::from_std_file(
                file.try_clone()
                    .map_err(|cause| fs::error(op, cause))?
                    .into_std(),
            )),
        }
    }
}

pub(super) struct LocalLookup {
    root: Arc<Dir>,
    directories: Arc<DirectoryBindings<Dir>>,
    operation: Operation,
    scope: LookupScope,
    observed: BTreeMap<StoragePath, Selected>,
    missing: BTreeSet<StoragePath>,
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
            scope: LookupScope::default(),
            observed: BTreeMap::new(),
            missing: BTreeSet::new(),
        }
    }

    fn selected(&self, entry: &ObservedEntry) -> Result<Selected, StorageError> {
        self.scope
            .get::<Selected>(entry)
            .cloned()
            .ok_or_else(|| stale(self.operation))
    }

    fn slot(
        &self,
        op: Operation,
        slot: EntrySlot<'_>,
        path: &StoragePath,
    ) -> Result<Slot, StorageError> {
        let parent = self.selected(slot.parent)?;
        let entry = slot.entry.map(|entry| self.selected(entry)).transpose()?;
        slot.validate(op, path)?;
        if self.operation != op || (entry.is_none() && !self.missing.contains(path)) {
            return Err(invalid(op));
        }
        Ok(Slot {
            parent: parent.directory(&self.root, op)?,
            name: slot.name.clone(),
            entry,
        })
    }

    fn target(
        &self,
        op: Operation,
        slot: Option<EntrySlot<'_>>,
        entry: &ObservedEntry,
        path: &StoragePath,
    ) -> Result<Selected, StorageError> {
        let selected = self.selected(entry)?;
        if self.operation != op || entry.entry().path() != path || slot.is_none() != path.is_root()
        {
            return Err(invalid(op));
        }
        if let Some(slot) = slot {
            self.slot(op, slot, path)?;
            if !slot
                .entry
                .is_some_and(|expected| std::ptr::eq(expected, entry))
            {
                return Err(invalid(op));
            }
        }
        Ok(selected)
    }

    fn checked(
        &self,
        mutable_name: Option<&StoragePath>,
    ) -> StorageFuture<'_, Vec<(StoragePath, Dir)>> {
        let op = self.operation;
        let root = self.root.clone();
        let observed = self.observed.clone();
        let mutable_name = mutable_name.cloned();
        Box::pin(blocking(op, false, move || {
            let mut pinned = BTreeMap::new();
            pinned.insert(
                StoragePath::root(),
                root.try_clone().map_err(|cause| fs::error(op, cause))?,
            );
            for (path, expected) in observed {
                let Some(parent) = path.parent() else {
                    continue;
                };
                let name = path.name().ok_or_else(|| invalid(op))?;
                let dir = pinned.get(&parent).ok_or_else(|| invalid(op))?;
                let (file, metadata) =
                    open(op, dir, name.as_str()).map_err(|error| changed(op, error))?;
                let original = expected.file.ok_or_else(|| invalid(op))?;
                let current = fs::entry(op, path.clone(), metadata)?;
                if identity(op, file.as_raw_handle())? != identity(op, original.as_raw_handle())?
                    || std::mem::discriminant(&current.entry_type())
                        != std::mem::discriminant(&expected.entry.entry_type())
                {
                    return Err(rejected(op, Rejection::Conflict));
                }
                if expected.entry.entry_type() == EntryType::Directory {
                    let shares_delete = mutable_name.as_ref().is_some_and(|mutable| {
                        mutable == &path
                            || path
                                .as_str()
                                .strip_prefix(mutable.as_str())
                                .is_some_and(|suffix| suffix.starts_with('/'))
                    });
                    let directory = if shares_delete {
                        Dir::from_std_file(file.into_std())
                    } else {
                        let pinned_directory = reopen_directory(op, file.as_raw_handle(), false)?;
                        let current = open(op, dir, name.as_str())
                            .map_err(|error| changed(op, error))?
                            .0;
                        if identity(op, current.as_raw_handle())?
                            != identity(op, pinned_directory.as_raw_handle())?
                        {
                            return Err(rejected(op, Rejection::Conflict));
                        }
                        pinned_directory
                    };
                    pinned.insert(path, directory);
                }
            }
            Ok(pinned.into_iter().collect())
        }))
    }
}

struct Slot {
    parent: Dir,
    name: EntryName,
    entry: Option<Selected>,
}

impl Slot {
    fn selected_file(&self, op: Operation) -> Result<&File, StorageError> {
        self.entry
            .as_ref()
            .and_then(|entry| entry.file.as_deref())
            .ok_or_else(|| invalid(op))
    }

    fn mutation_file(&self, op: Operation) -> Result<File, StorageError> {
        let observed = self.selected_file(op)?;
        let file = native::mutation_handle(op, &self.parent, self.name.as_str())
            .map_err(|error| changed(op, error))?;
        if identity(op, observed.as_raw_handle())? != identity(op, file.as_raw_handle())? {
            return Err(rejected(op, Rejection::Conflict));
        }
        Ok(file)
    }
}

impl OperationLookup for LocalLookup {
    fn operation(&self) -> Operation {
        self.operation
    }

    fn root(&mut self) -> StorageFuture<'_, ObservedEntry> {
        let root = self.root.clone();
        let op = self.operation;
        Box::pin(async move {
            let entry = blocking(op, false, move || {
                fs::entry(
                    op,
                    StoragePath::root(),
                    root.dir_metadata().map_err(|cause| fs::error(op, cause))?,
                )
            })
            .await?;
            Ok(self
                .scope
                .bind(entry.clone(), Selected { entry, file: None }))
        })
    }

    fn observe<'a>(
        &'a mut self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            let op = self.operation;
            let path = parent
                .entry()
                .path()
                .join(name)
                .map_err(|_| rejected(op, Rejection::PathTooLong))?;
            let dir = self.selected(parent)?.directory(&self.root, op)?;
            let name = name.clone();
            let address = path.clone();
            let result = blocking(op, false, move || {
                let (file, metadata) = open(op, &dir, name.as_str())?;
                let entry = fs::entry(op, address, metadata)?;
                Ok(Selected {
                    entry,
                    file: Some(Arc::new(file)),
                })
            })
            .await;
            let entry = match result {
                Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => {
                    if self.observed.contains_key(&path) {
                        return Err(rejected(op, Rejection::Conflict));
                    }
                    self.missing.insert(path);
                    return Err(error);
                }
                result => result?,
            };
            if self.missing.contains(&path) {
                return Err(rejected(op, Rejection::Conflict));
            }
            if let Some(previous) = self.observed.get(&path) {
                let before = previous.file.clone().ok_or_else(|| invalid(op))?;
                let after = entry.file.clone().ok_or_else(|| invalid(op))?;
                blocking(op, false, move || {
                    if identity(op, before.as_raw_handle())? != identity(op, after.as_raw_handle())?
                    {
                        return Err(rejected(op, Rejection::Conflict));
                    }
                    Ok(())
                })
                .await?;
            }
            self.observed.insert(path, entry.clone());
            Ok(self.scope.bind(entry.entry.clone(), entry))
        })
    }

    fn read_link<'a>(&'a self, entry: &'a ObservedEntry) -> StorageFuture<'a, LinkTarget> {
        Box::pin(async move {
            let selected = self.selected(entry)?;
            // Windows reparse data has no lossless logical-link representation here.
            let reason = if selected.entry.entry_type() == EntryType::Symlink {
                Rejection::Unsupported
            } else {
                Rejection::NotSymlink
            };
            Err(rejected(self.operation, reason))
        })
    }

    fn verify(&self) -> StorageFuture<'_, ()> {
        Box::pin(async move { self.checked(None).await.map(drop) })
    }

    fn execute_stat<'a>(
        &'a self,
        slot: Option<EntrySlot<'a>>,
        entry: &'a ObservedEntry,
        request: StatRequest,
    ) -> StorageFuture<'a, Entry> {
        Box::pin(async move {
            let entry = self.target(Operation::Stat, slot, entry, request.path())?;
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
            let slot = self.slot(op, slot, request.path())?;
            let entry = slot.entry.as_ref().ok_or_else(|| invalid(op))?;
            request
                .check_target(entry.entry.entry_type())
                .map_err(|reason| rejected(op, reason))?;
            self.verify().await?;
            blocking(op, false, move || {
                let file = open_reader(slot.selected_file(op)?)?;
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
            let slot = self.slot(op, slot, request.path())?;
            request
                .check_target(slot.entry.as_ref().map(|entry| entry.entry.entry_type()))
                .map_err(|reason| rejected(op, reason))?;
            let ancestors = self.checked(None).await?;
            blocking(op, false, move || {
                match open(op, &slot.parent, slot.name.as_str()) {
                    Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => {}
                    Ok(_) => return Err(rejected(op, Rejection::Conflict)),
                    Err(error) => return Err(error),
                }
                let destination = WriteDestination {
                    parent: slot.parent,
                    path: request.path().clone(),
                    ancestors,
                };
                Ok(Box::new(LocalFileWriter::open(destination, request)?)
                    as Box<dyn StorageFileWriter>)
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
            let slot = self.slot(op, slot, request.path())?;
            request
                .check_target(slot.entry.as_ref().map(|entry| entry.entry.entry_type()))
                .map_err(|reason| rejected(op, reason))?;
            let ancestors = self.checked(None).await?;
            let path = request.path().clone();
            self.missing.remove(&path);
            let selected = blocking(op, true, move || {
                let _ancestors = ancestors;
                slot.parent
                    .create_dir(slot.name.as_str())
                    .map_err(|cause| {
                        use std::io::ErrorKind;
                        match cause.kind() {
                            ErrorKind::AlreadyExists
                            | ErrorKind::NotFound
                            | ErrorKind::NotADirectory
                            | ErrorKind::PermissionDenied
                            | ErrorKind::InvalidInput
                            | ErrorKind::StorageFull
                            | ErrorKind::QuotaExceeded
                            | ErrorKind::ReadOnlyFilesystem => fs::error(op, cause),
                            _ => fs::dispatched_error(op, cause),
                        }
                    })?;
                let result = (|| {
                    let (file, metadata) = open(op, &slot.parent, slot.name.as_str())?;
                    if !metadata.is_dir() || fs::reparse(&metadata) {
                        return Err(StorageError::without_effects(
                            op,
                            Failure::Backend(BackendFailure::Other),
                            None,
                        ));
                    }
                    let entry = fs::entry(op, path, metadata)?;
                    Ok(Selected {
                        entry,
                        file: Some(Arc::new(file)),
                    })
                })();
                result.map_err(|error| {
                    StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
                })
            })
            .await?;
            self.observed
                .insert(selected.entry.path().clone(), selected.clone());
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
            let slot = self.slot(op, slot, request.path())?;
            let entry = slot.entry.as_ref().ok_or_else(|| invalid(op))?;
            request
                .check_target(entry.entry.entry_type())
                .map_err(|reason| rejected(op, reason))?;
            let ancestors = self.checked(Some(request.path())).await?;
            blocking(op, true, move || {
                let _ancestors = ancestors;
                native::remove(&slot.mutation_file(op)?)
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
            let source = self.slot(op, source, request.source())?;
            let destination = self.slot(op, destination, request.destination())?;
            let ancestors = self.checked(Some(request.source())).await?;
            blocking(op, true, move || {
                let _ancestors = ancestors;
                rename(source, destination, request)
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
            let entry = self.target(op, slot, entry, request.directory())?;
            if entry.entry.entry_type() != EntryType::Directory || request.after().is_some() {
                return Err(invalid(op));
            }
            self.verify().await?;
            let root = self.root.clone();
            let bindings = self.directories.clone();
            blocking(op, false, move || {
                let handle = entry
                    .file
                    .as_ref()
                    .map_or_else(|| root.as_raw_handle(), |file| file.as_raw_handle());
                let dir = reopen_directory(op, handle, true)?;
                bindings.bind(request.directory().clone(), dir, same_directory)
            })
            .await
        })
    }
}

fn rename(source: Slot, destination: Slot, request: RenameRequest) -> Result<Entry, StorageError> {
    let op = Operation::Rename;
    let selected = source.entry.as_ref().ok_or_else(|| invalid(op))?;
    let kind = selected.entry.entry_type();
    let same_slot = source.name == destination.name
        && identity(op, source.parent.as_raw_handle())?
            == identity(op, destination.parent.as_raw_handle())?;
    let observation = match &destination.entry {
        None => RenameDestination::Missing,
        Some(target)
            if !same_slot
                && request.mode() == RenameMode::Replace
                && kind == EntryType::Directory
                && target.entry.entry_type() == EntryType::Directory
                && !empty_directory(destination.selected_file(op)?)? =>
        {
            RenameDestination::NonEmptyDirectory
        }
        Some(target) => RenameDestination::Occupied(target.entry.entry_type()),
    };
    request
        .decide_destination(kind, observation)
        .map_err(|reason| rejected(op, reason))?;
    let file = source.mutation_file(op)?;
    let result = fs::entry(
        op,
        request.destination().clone(),
        file.metadata().map_err(|cause| fs::error(op, cause))?,
    )?;
    if same_slot {
        return Ok(result);
    }
    if observation != RenameDestination::Missing {
        // FileRenameInfo cannot conditionally replace an identified existing entry.
        return Err(rejected(op, Rejection::Unsupported));
    }
    match open_named(op, &destination.parent, destination.name.as_str()) {
        Ok(_) => return Err(rejected(op, Rejection::Conflict)),
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => {}
        Err(error) => return Err(error),
    }
    native::rename(op, &file, &destination.parent, destination.name.as_str()).map_err(|error| {
        if error.failure() == Failure::Rejected(Rejection::AlreadyExists)
            && request.mode() == RenameMode::Replace
        {
            rejected(op, Rejection::Conflict)
        } else {
            error
        }
    })?;
    Ok(result)
}

fn open_reader(selected: &File) -> Result<File, StorageError> {
    let op = Operation::Read;
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

fn empty_directory(file: &File) -> Result<bool, StorageError> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{FILE_LIST_DIRECTORY, ReOpenFile},
    };
    let op = Operation::Rename;
    // SAFETY: the selected object remains owned by file; a successful call returns a new handle.
    let handle = unsafe {
        ReOpenFile(
            file.as_raw_handle(),
            FILE_LIST_DIRECTORY | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(fs::error(op, std::io::Error::last_os_error()));
    }
    // SAFETY: handle is the newly owned directory handle from ReOpenFile.
    let dir = Dir::from_std_file(unsafe { std::fs::File::from_raw_handle(handle) });
    Ok(dir
        .entries()
        .map_err(|cause| fs::error(op, cause))?
        .next()
        .transpose()
        .map_err(|cause| fs::error(op, cause))?
        .is_none())
}

fn open(op: Operation, dir: &Dir, name: &str) -> Result<(File, Metadata), StorageError> {
    let (file, metadata, actual) = open_named(op, dir, name)?;
    if actual != name {
        return Err(rejected(op, Rejection::NotFound));
    }
    Ok((file, metadata))
}

fn open_named(
    op: Operation,
    dir: &Dir,
    name: &str,
) -> Result<(File, Metadata, String), StorageError> {
    fs::validate_name(op, name)?;
    let mut options = OpenOptions::new();
    let share = if op == Operation::Write {
        FILE_SHARE_READ | FILE_SHARE_DELETE
    } else {
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
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
    let actual = native::actual_name(op, &file)?;
    let metadata = file.metadata().map_err(|cause| fs::error(op, cause))?;
    Ok((file, metadata, actual))
}

pub(super) struct WriteDestination {
    pub parent: Dir,
    pub path: StoragePath,
    // Ancestors were opened without delete sharing and remain bound for publication.
    pub ancestors: Vec<(StoragePath, Dir)>,
}

fn changed(op: Operation, error: StorageError) -> StorageError {
    match error.failure() {
        Failure::Rejected(Rejection::NotFound | Rejection::NotDirectory) => {
            rejected(op, Rejection::Conflict)
        }
        _ => error,
    }
}

fn identity(op: Operation, handle: RawHandle) -> Result<(u64, [u8; 16]), StorageError> {
    let mut data = std::mem::MaybeUninit::<FILE_ID_INFO>::uninit();
    // SAFETY: The handle is live and the output is writable for the call.
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            data.as_mut_ptr().cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(fs::error(op, std::io::Error::last_os_error()));
    }
    // SAFETY: A successful query initialized the output.
    let data = unsafe { data.assume_init() };
    Ok((data.VolumeSerialNumber, data.FileId.Identifier))
}

fn reopen_directory(
    op: Operation,
    handle: RawHandle,
    share_delete: bool,
) -> Result<Dir, StorageError> {
    let share =
        FILE_SHARE_READ | FILE_SHARE_WRITE | if share_delete { FILE_SHARE_DELETE } else { 0 };
    let access = if share_delete {
        FILE_GENERIC_READ
    } else {
        FILE_READ_ATTRIBUTES | SYNCHRONIZE
    };
    // SAFETY: The original handle stays live and success transfers ownership of a new handle.
    let handle = unsafe {
        ReOpenFile(
            handle,
            access,
            share,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(fs::error(op, std::io::Error::last_os_error()));
    }
    // SAFETY: ReOpenFile returned a new owned directory handle.
    Ok(Dir::from_std_file(unsafe {
        std::fs::File::from_raw_handle(handle)
    }))
}

pub(super) fn same_directory(left: &Dir, right: &Dir) -> Result<bool, StorageError> {
    let op = Operation::List;
    Ok(identity(op, left.as_raw_handle())? == identity(op, right.as_raw_handle())?)
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
    selection: &DirectorySelection,
    resuming: bool,
) -> Result<Dir, StorageError> {
    let op = Operation::List;
    let mut directory = root.try_clone().map_err(|cause| fs::error(op, cause))?;
    for name in selection.address().entry_names() {
        let (file, metadata) =
            open(op, &directory, name.as_str()).map_err(|error| match error.failure() {
                Failure::Rejected(Rejection::NotFound | Rejection::NotDirectory) => {
                    list_replaced(resuming)
                }
                _ => error,
            })?;
        if !metadata.is_dir() || fs::reparse(&metadata) {
            return Err(list_replaced(resuming));
        }
        directory = Dir::from_std_file(file.into_std());
    }
    reopen_directory(op, directory.as_raw_handle(), true)
}
