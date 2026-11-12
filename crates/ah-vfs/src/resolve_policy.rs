//! Lookup rules derived from a concrete operation request.
use super::ResolveFailure;
use super::path::PathExpr;
use ah_vfs_storage::{EntryType, Rejection, StoragePath};

/// The execution context selects a policy for links before the final component.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IntermediateSymlinks {
    /// Relative link targets start at the containing directory. Absolute link
    /// targets start at the instance root. All traversal stays in the instance.
    #[default]
    FollowWithinInstance,
    Reject,
}

/// Whether to select a link's target or the link itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymlinkAction {
    FollowWithinInstance,
    Preserve,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExistingTarget {
    Any,
    Absent,
    Directory,
    RegularFile,
    NonDirectory,
}

/// Where an absent ordinary name was observed in the walk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingName {
    OperandIntermediate,
    OperandFinal,
    /// Any component in a followed target, including nested expansions.
    LinkTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MissingPolicy {
    Reject,
    Final,
    Recursive,
}

/// A borrowed view of an operation's path operand and lookup requirements.
/// For a relative operand, the executor selects a starting directory from the
/// base address supplied for this operation.
///
/// Walk components in order. Apply the directory requirement of the operand
/// even when the operation preserves the final link. Check the address and
/// existing target as part of the operation against the selected entry or
/// parent directory and name. A check against a string alone does not secure
/// subsequent I/O against concurrent replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathLookup<'a> {
    path: &'a PathExpr,
    final_symlink: SymlinkAction,
    existing: ExistingTarget,
    protect_root: bool,
    missing: MissingPolicy,
}

impl<'a> PathLookup<'a> {
    pub(super) fn new(
        path: &'a PathExpr,
        final_symlink: SymlinkAction,
        existing: ExistingTarget,
    ) -> Self {
        Self {
            path,
            final_symlink,
            existing,
            protect_root: false,
            missing: MissingPolicy::Reject,
        }
    }

    pub(super) fn protecting_root(mut self) -> Self {
        self.protect_root = true;
        self
    }

    pub(super) fn with_missing(mut self, policy: MissingPolicy) -> Self {
        self.missing = policy;
        self
    }

    pub fn path(self) -> &'a PathExpr {
        self.path
    }

    pub fn final_symlink(self) -> SymlinkAction {
        self.final_symlink
    }

    pub fn requires_directory(self) -> bool {
        self.path.requires_directory() || self.existing == ExistingTarget::Directory
    }

    /// Check the selected address, including an as-yet unused destination.
    /// Root protection applies to the actual target after traversal.
    pub fn check_address(self, address: &StoragePath) -> Result<(), ResolveFailure> {
        if self.protect_root && address.is_root() {
            return Err(ResolveFailure::RootProtected);
        }
        Ok(())
    }

    /// Check whether an absent name may be created at the selected address.
    /// For an absent final write or rename destination, use the operation's
    /// destination check to also validate the proposed entry kind.
    pub fn check_missing_name(
        self,
        address: &StoragePath,
        name: MissingName,
    ) -> Result<(), ResolveFailure> {
        match (self.missing, name) {
            (MissingPolicy::Final | MissingPolicy::Recursive, MissingName::OperandFinal)
            | (MissingPolicy::Recursive, MissingName::OperandIntermediate) => {
                self.check_address(address)
            }
            _ => Err(Rejection::NotFound.into()),
        }
    }

    /// Check an observed target after applying the final-link policy.
    /// Operations that accept a missing target must handle its absence
    /// according to their own mode.
    pub fn check_existing_target(
        self,
        address: &StoragePath,
        entry_type: EntryType,
    ) -> Result<(), ResolveFailure> {
        self.check_address(address)?;
        self.check_directory(entry_type == EntryType::Directory)?;
        match self.existing {
            ExistingTarget::Absent => Err(Rejection::AlreadyExists.into()),
            ExistingTarget::RegularFile => match entry_type {
                EntryType::File { .. } => Ok(()),
                EntryType::Directory => Err(Rejection::IsDirectory.into()),
                _ => Err(Rejection::NotRegularFile.into()),
            },
            ExistingTarget::NonDirectory if entry_type == EntryType::Directory => {
                Err(Rejection::IsDirectory.into())
            }
            ExistingTarget::Any | ExistingTarget::Directory | ExistingTarget::NonDirectory => {
                Ok(())
            }
        }
    }

    pub fn check_missing_target_shape(
        self,
        address: &StoragePath,
        is_directory: bool,
    ) -> Result<(), ResolveFailure> {
        self.check_missing_name(address, MissingName::OperandFinal)?;
        self.check_directory(is_directory)
    }

    fn check_directory(self, is_directory: bool) -> Result<(), ResolveFailure> {
        if self.requires_directory() && !is_directory {
            return Err(Rejection::NotDirectory.into());
        }
        Ok(())
    }
}
