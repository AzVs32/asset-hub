//! Path operands and policies used to resolve VFS operations.
use super::ResolveFailure;
use super::{
    path::PathExpr,
    policy::{ExistingTarget, MissingPolicy, PathLookup, SymlinkAction},
};
use ah_vfs_storage::{
    BackendFailure, BoundListRequest, DirectoryCursor, DirectorySelection, PageSize, Publication,
    ReadRange, RemoveKind, RenameMode, StoragePath, WriteMode,
};
use getset::{CopyGetters, Getters};
use thiserror::Error;

/// Inspect the selected entry or the target of its final symbolic link.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct StatOperand {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    final_symlink: SymlinkAction,
}

impl StatOperand {
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

/// Read bytes from an existing regular file, following the final link.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct ReadOperand {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    range: ReadRange,
}

impl ReadOperand {
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
pub struct WriteOperand {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    mode: WriteMode,
    #[getset(get_copy = "pub")]
    publication: Publication,
}

impl WriteOperand {
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
    pub fn check_new_destination(&self, address: &StoragePath) -> Result<(), ResolveFailure> {
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
pub struct CreateDirectoryOperand {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    mode: CreateDirectoryMode,
}

impl CreateDirectoryOperand {
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
pub enum OperandError {
    #[error("the path operand must end in an entry name")]
    ExpectedEntryName,
}

fn require_entry_name(path: &PathExpr) -> Result<(), OperandError> {
    if path.ends_in_name() {
        Ok(())
    } else {
        Err(OperandError::ExpectedEntryName)
    }
}

/// Remove one selected directory entry, preserving any final link as that entry.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct RemoveOperand {
    #[getset(get = "pub")]
    path: PathExpr,
    #[getset(get_copy = "pub")]
    kind: RemoveKind,
}

impl RemoveOperand {
    pub fn new(path: PathExpr, kind: RemoveKind) -> Result<Self, OperandError> {
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

/// Atomic rename within one instance, never copy/delete. Both roots are excluded.
/// Both operands preserve the final symbolic link as a directory entry. Resolve
/// the source first and use the same instance, per-call base address and link
/// traversal budget for both. An absent destination with a directory
/// requirement can only be used for a directory source.
/// Once both operands pass lookup, compare directory entry slots by their
/// parent identities and names. The same slot fails as AlreadyExists in
/// NoReplace mode and succeeds without changes in Replace mode. An absent
/// source fails as NotFound before any destination change.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct RenameOperand {
    #[getset(get = "pub")]
    source: PathExpr,
    #[getset(get = "pub")]
    destination: PathExpr,
    #[getset(get_copy = "pub")]
    mode: RenameMode,
}

impl RenameOperand {
    pub fn new(
        source: PathExpr,
        destination: PathExpr,
        mode: RenameMode,
    ) -> Result<Self, OperandError> {
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListOperandTarget {
    /// Resolve a fresh operand under the operation's lookup context.
    Path(PathExpr),
    /// Resume at the selected address and directory identity in the cursor.
    Continue(DirectoryCursor),
}

/// List immediate children in exact UTF-8 name order. A continuation keeps the
/// selected address and identity. Enumeration is live, so directory contents
/// can change between pages.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct ListOperand {
    #[getset(get = "pub")]
    target: ListOperandTarget,
    #[getset(get_copy = "pub")]
    size: PageSize,
}

impl ListOperand {
    pub fn new(target: ListOperandTarget, size: PageSize) -> Self {
        Self { target, size }
    }

    /// Lookup for an initial request. A continuation uses its bound selection
    /// instead of the original operand or working directory.
    pub fn lookup(&self) -> Option<PathLookup<'_>> {
        match &self.target {
            ListOperandTarget::Path(path) => Some(PathLookup::new(
                path,
                SymlinkAction::FollowWithinInstance,
                ExistingTarget::Directory,
            )),
            ListOperandTarget::Continue(_) => None,
        }
    }

    /// Build the canonical request using the directory selected for this operation.
    pub fn bind_selection(
        &self,
        selection: DirectorySelection,
    ) -> Result<BoundListRequest, BackendFailure> {
        let request = match &self.target {
            ListOperandTarget::Path(_) => {
                ah_vfs_storage::ListRequest::new(selection.address().clone(), self.size)
            }
            ListOperandTarget::Continue(cursor) => {
                ah_vfs_storage::ListRequest::continuing(cursor.clone(), self.size)
            }
        };
        request.bind_selection(selection)
    }
}
