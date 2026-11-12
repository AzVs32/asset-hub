//! Validated requests and operation identities. Execution and streams live in ports.
use super::{EntryType, Rejection, StoragePath};
use getset::{CopyGetters, Getters};
use std::fmt;
use thiserror::Error;

/// The logical operation. Staging, commit, abort and cleanup of a write all
/// belong to `Write`. Ports track the phase and evidence for each attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Operation {
    Stat,
    List,
    Read,
    ReadLink,
    Write,
    CreateDirectory,
    Remove,
    Rename,
    GetPermissions,
    SetPermissions,
    ListXattrs,
    GetXattr,
    SetXattr,
    RemoveXattr,
}
impl Operation {
    /// Whether this operation reads or changes permissions or extended attributes.
    pub fn is_attribute(self) -> bool {
        matches!(
            self,
            Self::GetPermissions
                | Self::SetPermissions
                | Self::ListXattrs
                | Self::GetXattr
                | Self::SetXattr
                | Self::RemoveXattr
        )
    }

    /// Future operations conservatively count as mutations until explicitly classified.
    pub fn may_mutate(self) -> bool {
        !matches!(
            self,
            Self::Stat
                | Self::List
                | Self::Read
                | Self::ReadLink
                | Self::GetPermissions
                | Self::ListXattrs
                | Self::GetXattr
        )
    }
}
impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stat => "stat",
            Self::List => "list",
            Self::Read => "read",
            Self::ReadLink => "read link",
            Self::Write => "write",
            Self::CreateDirectory => "create directory",
            Self::Remove => "remove",
            Self::Rename => "rename",
            Self::GetPermissions => "get permissions",
            Self::SetPermissions => "set permissions",
            Self::ListXattrs => "list attributes",
            Self::GetXattr => "get attribute",
            Self::SetXattr => "set attribute",
            Self::RemoveXattr => "remove attribute",
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum RangeError {
    #[error("exclusive range end exceeds the addressable byte range")]
    Overflow,
}
/// Byte offsets are never normalized. Zero length and reads beyond EOF yield
/// empty results; absent length means through EOF. Separate reads are not snapshots.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, CopyGetters)]
#[getset(get_copy = "pub")]
pub struct ReadRange {
    offset: u64,
    length: Option<u64>,
}
impl ReadRange {
    pub const FULL: Self = Self {
        offset: 0,
        length: None,
    };
    pub fn new(offset: u64, length: Option<u64>) -> Result<Self, RangeError> {
        if let Some(length) = length {
            offset.checked_add(length).ok_or(RangeError::Overflow)?;
        }
        Ok(Self { offset, length })
    }
}
/// Inspect the entry at a canonical address.
#[derive(Clone, Debug, Eq, PartialEq, Getters)]
pub struct StatRequest {
    #[getset(get = "pub")]
    path: StoragePath,
}

impl StatRequest {
    pub fn new(path: StoragePath) -> Self {
        Self { path }
    }
}

/// Read the final link's exact stored bytes as [`super::LinkTarget`], including
/// a dangling link. The result does not require UTF-8 or path validation.
#[derive(Clone, Debug, Eq, PartialEq, Getters)]
pub struct ReadLinkRequest {
    #[getset(get = "pub")]
    path: StoragePath,
}

impl ReadLinkRequest {
    pub fn new(path: StoragePath) -> Self {
        Self { path }
    }

    pub fn check_target(&self, kind: EntryType) -> Result<(), Rejection> {
        if kind == EntryType::Symlink {
            Ok(())
        } else {
            Err(Rejection::NotSymlink)
        }
    }
}

/// Read bytes from the regular file at the selected address.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct ReadRequest {
    #[getset(get = "pub")]
    path: StoragePath,
    #[getset(get_copy = "pub")]
    range: ReadRange,
}

impl ReadRequest {
    pub fn new(path: StoragePath, range: ReadRange) -> Self {
        Self { path, range }
    }

    pub fn check_target(&self, kind: EntryType) -> Result<(), Rejection> {
        check_regular_file(kind)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteMode {
    /// No destination entry may exist when the write is opened or published.
    CreateNew,
    /// Publish a complete file. An existing destination must be a regular file.
    Replace,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Publication {
    /// Stage until commit. A failed publication may leave visible partial effects.
    Staged,
    /// Commit makes the complete file visible atomically. A lost commit reply
    /// still leaves publication uncertain. This does not promise crash durability.
    Atomic,
}
/// Stage a complete file at a canonical name and publish on commit.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct WriteRequest {
    #[getset(get = "pub")]
    path: StoragePath,
    #[getset(get_copy = "pub")]
    mode: WriteMode,
    #[getset(get_copy = "pub")]
    publication: Publication,
}

impl WriteRequest {
    pub fn new(
        path: StoragePath,
        mode: WriteMode,
        publication: Publication,
    ) -> Result<Self, RequestPathError> {
        require_non_root(&path)?;
        Ok(Self {
            path,
            mode,
            publication,
        })
    }

    pub fn check_target(&self, observed: Option<EntryType>) -> Result<(), Rejection> {
        match observed {
            None => Ok(()),
            Some(_) if self.mode == WriteMode::CreateNew => Err(Rejection::AlreadyExists),
            Some(kind) => check_regular_file(kind),
        }
    }
}

fn check_regular_file(kind: EntryType) -> Result<(), Rejection> {
    match kind {
        EntryType::File { .. } => Ok(()),
        EntryType::Directory => Err(Rejection::IsDirectory),
        EntryType::Symlink | EntryType::Other => Err(Rejection::NotRegularFile),
    }
}

/// Create one absent directory. Its parent must already exist.
#[derive(Clone, Debug, Eq, PartialEq, Getters)]
pub struct CreateDirectoryRequest {
    #[getset(get = "pub")]
    path: StoragePath,
}

impl CreateDirectoryRequest {
    pub fn new(path: StoragePath) -> Result<Self, RequestPathError> {
        require_non_root(&path)?;
        Ok(Self { path })
    }

    pub fn check_target(&self, observed: Option<EntryType>) -> Result<(), Rejection> {
        match observed {
            None => Ok(()),
            Some(_) => Err(Rejection::AlreadyExists),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum RequestPathError {
    #[error("the storage root cannot be modified by this request")]
    RootNotAllowed,
}

fn require_non_root(path: &StoragePath) -> Result<(), RequestPathError> {
    if path.is_root() {
        Err(RequestPathError::RootNotAllowed)
    } else {
        Ok(())
    }
}

/// The kind of directory entry to remove. Both forms remove one entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoveKind {
    /// A file, link itself, or other non-directory entry.
    NonDirectory,
    EmptyDirectory,
}

/// Remove one selected entry. Removing a directory requires it to be empty.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct RemoveRequest {
    #[getset(get = "pub")]
    path: StoragePath,
    #[getset(get_copy = "pub")]
    kind: RemoveKind,
}

impl RemoveRequest {
    pub fn new(path: StoragePath, kind: RemoveKind) -> Result<Self, RequestPathError> {
        require_non_root(&path)?;
        Ok(Self { path, kind })
    }

    pub fn check_target(&self, kind: EntryType) -> Result<(), Rejection> {
        match (self.kind, kind == EntryType::Directory) {
            (RemoveKind::NonDirectory, true) => Err(Rejection::IsDirectory),
            (RemoveKind::EmptyDirectory, false) => Err(Rejection::NotDirectory),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenameMode {
    NoReplace,
    Replace,
}

/// The observation at a distinct rename destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenameDestination {
    Missing,
    /// A directory in this variant is empty.
    Occupied(EntryType),
    NonEmptyDirectory,
}

/// Atomically rename between two canonical, non-root addresses.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct RenameRequest {
    #[getset(get = "pub")]
    source: StoragePath,
    #[getset(get = "pub")]
    destination: StoragePath,
    #[getset(get_copy = "pub")]
    mode: RenameMode,
}

impl RenameRequest {
    pub fn new(
        source: StoragePath,
        destination: StoragePath,
        mode: RenameMode,
    ) -> Result<Self, RequestPathError> {
        require_non_root(&source)?;
        require_non_root(&destination)?;
        Ok(Self {
            source,
            destination,
            mode,
        })
    }

    pub fn decide_destination(
        &self,
        source: EntryType,
        destination: RenameDestination,
    ) -> Result<(), Rejection> {
        let kind = match destination {
            RenameDestination::Missing => return Ok(()),
            _ if self.mode == RenameMode::NoReplace => return Err(Rejection::AlreadyExists),
            RenameDestination::Occupied(kind) => kind,
            RenameDestination::NonEmptyDirectory => EntryType::Directory,
        };
        match (source == EntryType::Directory, kind == EntryType::Directory) {
            (true, false) => Err(Rejection::NotDirectory),
            (false, true) => Err(Rejection::IsDirectory),
            _ if destination == RenameDestination::NonEmptyDirectory => {
                Err(Rejection::DirectoryNotEmpty)
            }
            _ => Ok(()),
        }
    }
}
