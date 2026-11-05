//! Validated requests and operation identities. Execution and streams live in ports.
use super::{EntryType, NonRootPath, Rejection, StoragePath};
use bon::{Builder, bon};
use getset::{CopyGetters, Getters};
use std::fmt;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Operation {
    Stat,
    List,
    Read,
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
    /// Future operations conservatively count as mutations until explicitly classified.
    pub fn may_mutate(self) -> bool {
        !matches!(
            self,
            Self::Stat
                | Self::List
                | Self::Read
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
#[bon]
impl ReadRange {
    pub const FULL: Self = Self {
        offset: 0,
        length: None,
    };
    #[builder(start_fn = builder, state_mod(vis = "pub"))]
    pub fn new(#[builder(default)] offset: u64, length: Option<u64>) -> Result<Self, RangeError> {
        if let Some(length) = length {
            offset.checked_add(length).ok_or(RangeError::Overflow)?;
        }
        Ok(Self { offset, length })
    }
    pub fn end_exclusive(self) -> Option<u64> {
        self.length.map(|length| self.offset + length)
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Builder, Getters, CopyGetters)]
#[builder(state_mod(vis = "pub"))]
pub struct ReadRequest {
    #[getset(get = "pub")]
    path: StoragePath,
    #[builder(default)]
    #[getset(get_copy = "pub")]
    range: ReadRange,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WriteMode {
    /// No destination entry may exist when the write is opened or published.
    CreateNew,
    /// Publish a complete file; only an existing regular file may be replaced.
    Replace,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Publication {
    /// Stage until commit, with no promise of atomic visibility during publication.
    Staged,
    /// Publish the complete file atomically. This does not promise crash durability.
    Atomic,
}
/// A whole-file write. Unsupported guarantees fail before staging. Commit/abort
/// are terminal, including failed attempts; drop must never publish staged data.
/// Ports must report uncertain publication as MutationOutcome::Unknown.
#[derive(Clone, Debug, Eq, PartialEq, Builder, Getters, CopyGetters)]
#[builder(state_mod(vis = "pub"))]
pub struct WriteRequest {
    #[getset(get = "pub")]
    path: NonRootPath,
    #[getset(get_copy = "pub")]
    mode: WriteMode,
    #[getset(get_copy = "pub")]
    publication: Publication,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CreateDirectoryMode {
    /// Parents must exist. Any existing target (file or directory), including
    /// root, fails as AlreadyExists.
    Single,
    /// Create missing parents. Existing directories including root are accepted;
    /// a non-directory target fails as NotDirectory.
    Recursive,
}
impl CreateDirectoryMode {
    /// Classify an existing target observed by the adapter, without performing I/O.
    /// This is not a check-then-create algorithm: adapters must enforce the same
    /// policy during the backend operation, including races with other callers.
    pub fn check_existing_target(self, entry_type: EntryType) -> Result<(), Rejection> {
        match self {
            Self::Single => Err(Rejection::AlreadyExists),
            Self::Recursive => match entry_type {
                EntryType::Directory => Ok(()),
                EntryType::File { .. } | EntryType::Symlink | EntryType::Other => {
                    Err(Rejection::NotDirectory)
                }
            },
        }
    }
}
/// Directory creation policy:
///
/// | Observed condition | Single | Recursive |
/// | --- | --- | --- |
/// | Existing directory target (including root) | AlreadyExists | Success |
/// | Existing non-directory target | AlreadyExists | NotDirectory |
/// | Non-directory in the parent chain | NotDirectory | NotDirectory |
/// | Absent parent directory | NotFound | Create missing parents |
///
/// Adapters classify existing targets using CreateDirectoryMode::check_existing_target.
/// Parent-chain failures are independent of the target's existence policy.
#[derive(Clone, Debug, Eq, PartialEq, Builder, Getters, CopyGetters)]
#[builder(state_mod(vis = "pub"))]
pub struct CreateDirectoryRequest {
    #[getset(get = "pub")]
    path: StoragePath,
    #[getset(get_copy = "pub")]
    mode: CreateDirectoryMode,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RenameMode {
    NoReplace,
    Replace,
}
/// Atomic rename within one instance, never copy/delete. Both roots are excluded.
/// For equal addresses, an absent source fails as NotFound; an existing source
/// fails as AlreadyExists in NoReplace mode and is unchanged in Replace mode.
#[derive(Clone, Debug, Eq, PartialEq, Builder, Getters, CopyGetters)]
#[builder(state_mod(vis = "pub"))]
pub struct RenameRequest {
    #[getset(get = "pub")]
    source: NonRootPath,
    #[getset(get = "pub")]
    destination: NonRootPath,
    #[getset(get_copy = "pub")]
    mode: RenameMode,
}
