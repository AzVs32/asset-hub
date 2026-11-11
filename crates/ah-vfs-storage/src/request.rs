//! Validated requests and operation identities. Execution and streams live in ports.
use super::{
    EntryType, PathExpr, PathLookup, Rejection, StoragePath, SymlinkAction,
    lookup::{ExistingTarget, MissingPolicy},
};
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
    pub fn end_exclusive(self) -> Option<u64> {
        self.length.map(|length| self.offset + length)
    }
}
/// Inspect the selected entry or the target of its final symbolic link.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct StatRequest {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    final_symlink: SymlinkAction,
}

impl StatRequest {
    pub fn new(path: PathExpr, final_symlink: SymlinkAction) -> Self {
        Self {
            path,
            final_symlink,
        }
    }

    pub fn lookup(&self) -> PathLookup<'_> {
        PathLookup::new(&self.path, self.final_symlink, ExistingTarget::Any)
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

/// Read bytes from an existing regular file, following the final link.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct ReadRequest {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    range: ReadRange,
}

impl ReadRequest {
    pub fn new(path: PathExpr, range: ReadRange) -> Self {
        Self { path, range }
    }

    pub fn lookup(&self) -> PathLookup<'_> {
        PathLookup::new(
            &self.path,
            SymlinkAction::FollowWithinInstance,
            ExistingTarget::RegularFile,
        )
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
/// A whole-file write. Reject unsupported guarantees before staging.
/// Stage privately until commit. After a staging failure, the writer may only
/// discard the stage or be dropped. Commit and abort are terminal even on
/// failure. Abort, cleanup and drop must never publish.
///
/// A staging or cleanup failure can be reported as unchanged when publication
/// is ruled out for the entire attempt. An uncertain commit or a known partial
/// publication requires [`super::MutationOutcome::Unknown`]. Cleaning up the
/// stage cannot downgrade the outcome of an earlier failed commit.
///
/// The destination is a regular-file name in the selected parent directory.
/// The final link is preserved, and a directory-only operand cannot be used
/// to publish a file even if the target name is absent.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct WriteRequest {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    mode: WriteMode,
    #[getset(get_copy = "pub")]
    publication: Publication,
}

impl WriteRequest {
    pub fn new(path: PathExpr, mode: WriteMode, publication: Publication) -> Self {
        Self {
            path,
            mode,
            publication,
        }
    }

    pub fn lookup(&self) -> PathLookup<'_> {
        let existing = match self.mode {
            WriteMode::CreateNew => ExistingTarget::Absent,
            WriteMode::Replace => ExistingTarget::RegularFile,
        };
        PathLookup::new(&self.path, SymlinkAction::Preserve, existing)
            .protecting_root()
            .with_missing(MissingPolicy::Final)
    }

    /// Check a destination already established to be absent in its parent.
    pub fn check_new_destination(&self, address: &StoragePath) -> Result<(), Rejection> {
        self.lookup().check_missing_target_shape(address, false)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateDirectoryMode {
    /// Parents must exist. Any existing target (file or directory), including
    /// root, fails as AlreadyExists.
    Single,
    /// Create missing parents. Existing directories including root are accepted;
    /// a non-directory target fails as NotDirectory.
    Recursive,
}
/// Directory creation policy:
///
/// | Observed condition | Single | Recursive |
/// | --- | --- | --- |
/// | Existing directory target (including root) | AlreadyExists | Success |
/// | Existing non-directory without a directory-only operand | AlreadyExists | NotDirectory |
/// | Non-directory in the parent chain | NotDirectory | NotDirectory |
/// | Absent parent directory | NotFound | Create missing parents |
///
/// An explicit directory requirement also applies to existing targets. Recursive
/// creation may create missing names from the original operand as it walks,
/// even before a subsequent `..`. Followed links must resolve to existing
/// targets. A missing component in any link target fails with `NotFound`,
/// including in recursive mode. Resume creation only in the original operand
/// after its link target has been fully resolved. Check operation support
/// before creating parents. A later failure after any parent became visible
/// requires [`super::MutationOutcome::Unknown`], even if the final target
/// was never reached or visible parents were later removed.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct CreateDirectoryRequest {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    mode: CreateDirectoryMode,
}

impl CreateDirectoryRequest {
    pub fn new(path: PathExpr, mode: CreateDirectoryMode) -> Self {
        Self { path, mode }
    }

    pub fn lookup(&self) -> PathLookup<'_> {
        let (link, existing, missing) = match self.mode {
            CreateDirectoryMode::Single => (
                SymlinkAction::Preserve,
                ExistingTarget::Absent,
                MissingPolicy::Final,
            ),
            CreateDirectoryMode::Recursive => (
                SymlinkAction::FollowWithinInstance,
                ExistingTarget::Directory,
                MissingPolicy::Recursive,
            ),
        };
        PathLookup::new(&self.path, link, existing).with_missing(missing)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum RequestPathError {
    #[error("the path operand must end in an entry name")]
    ExpectedEntryName,
}

fn require_entry_name(path: &PathExpr) -> Result<(), RequestPathError> {
    if path.ends_in_name() {
        Ok(())
    } else {
        Err(RequestPathError::ExpectedEntryName)
    }
}

/// The kind of directory entry to remove. Both forms remove one entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoveKind {
    /// A file, link itself, or other non-directory entry.
    NonDirectory,
    EmptyDirectory,
}

/// Remove one selected directory entry, preserving any final link as that entry.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct RemoveRequest {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    kind: RemoveKind,
}

impl RemoveRequest {
    pub fn new(path: PathExpr, kind: RemoveKind) -> Result<Self, RequestPathError> {
        require_entry_name(&path)?;
        Ok(Self { path, kind })
    }

    pub fn lookup(&self) -> PathLookup<'_> {
        let existing = match self.kind {
            RemoveKind::NonDirectory => ExistingTarget::NonDirectory,
            RemoveKind::EmptyDirectory => ExistingTarget::Directory,
        };
        PathLookup::new(&self.path, SymlinkAction::Preserve, existing).protecting_root()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenameMode {
    NoReplace,
    Replace,
}

/// The destination observed within the same bound operation as the source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenameDestination {
    Missing,
    /// The same directory entry slot, identified by its parent identity and name.
    /// A different name for the same underlying object is a distinct slot.
    SameSlot,
    /// An occupied, distinct slot. A directory in this variant is empty.
    Occupied(EntryType),
    NonEmptyDirectory,
}

/// Whether the executor needs to issue an atomic move for a validated rename.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenameDecision {
    Unchanged,
    Move,
}

/// Atomic rename within one instance, never copy/delete. Both roots are excluded.
/// Both operands preserve the final symbolic link as a directory entry. Resolve
/// the source first and use the same captured instance, working directory and
/// link traversal budget for both. An absent destination with a directory
/// requirement can only be used for a directory source.
/// Once both operands pass lookup, compare directory entry slots by their
/// parent identities and names. The same slot fails as AlreadyExists in
/// NoReplace mode and succeeds without changes in Replace mode. An absent
/// source fails as NotFound before any destination change.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct RenameRequest {
    #[getset(get = "pub")]
    source: PathExpr,
    #[getset(get = "pub")]
    destination: PathExpr,
    #[getset(get_copy = "pub")]
    mode: RenameMode,
}

impl RenameRequest {
    pub fn new(
        source: PathExpr,
        destination: PathExpr,
        mode: RenameMode,
    ) -> Result<Self, RequestPathError> {
        require_entry_name(&source)?;
        require_entry_name(&destination)?;
        Ok(Self {
            source,
            destination,
            mode,
        })
    }

    /// The source and destination, in that order, for one bound operation.
    pub fn lookups(&self) -> (PathLookup<'_>, PathLookup<'_>) {
        let existing = match self.mode {
            RenameMode::NoReplace => ExistingTarget::Absent,
            RenameMode::Replace => ExistingTarget::Any,
        };
        (
            PathLookup::new(&self.source, SymlinkAction::Preserve, ExistingTarget::Any)
                .protecting_root(),
            PathLookup::new(&self.destination, SymlinkAction::Preserve, existing)
                .protecting_root()
                .with_missing(MissingPolicy::Final),
        )
    }

    /// Decide the rename after successfully resolving its source and looking
    /// up its destination. Determine slot identity and directory emptiness
    /// within the same binding used for the atomic operation.
    pub fn decide_destination(
        &self,
        address: &StoragePath,
        source: EntryType,
        destination: RenameDestination,
    ) -> Result<RenameDecision, Rejection> {
        let lookup = self.lookups().1;
        let destination_kind = match destination {
            RenameDestination::Missing => {
                lookup.check_missing_target_shape(address, source == EntryType::Directory)?;
                return Ok(RenameDecision::Move);
            }
            RenameDestination::SameSlot => {
                lookup.check_existing_target(address, source)?;
                return Ok(RenameDecision::Unchanged);
            }
            RenameDestination::Occupied(kind) => kind,
            RenameDestination::NonEmptyDirectory => EntryType::Directory,
        };
        lookup.check_existing_target(address, destination_kind)?;
        match (
            source == EntryType::Directory,
            destination_kind == EntryType::Directory,
        ) {
            (true, false) => Err(Rejection::NotDirectory),
            (false, true) => Err(Rejection::IsDirectory),
            _ if destination == RenameDestination::NonEmptyDirectory => {
                Err(Rejection::DirectoryNotEmpty)
            }
            _ => Ok(RenameDecision::Move),
        }
    }
}
