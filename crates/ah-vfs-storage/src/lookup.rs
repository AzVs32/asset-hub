//! Lookup rules derived from a concrete operation request.
use super::{
    BackendFailure, EntryType, Failure, LookupStart, MAX_PATH_BYTES, MAX_PATH_EXPR_BYTES,
    NameError, PathComponent, PathExpr, PathExprError, Rejection, StoragePath,
};

/// Maximum number of link expansions in one operation, across both operands
/// of a rename and any links encountered inside other links' targets.
pub const MAX_SYMLINK_EXPANSIONS: u8 = 40;

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

/// The exact bytes stored in a symbolic link. Reading a link preserves its
/// contents even when they cannot be interpreted as a storage path operand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkTarget(Vec<u8>);

impl LinkTarget {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Prepare a followed link's target. Interpret a relative target from the
    /// containing directory's identity and an absolute target from the bound
    /// instance root. `pending` contains all unvisited components after the
    /// link, including suffixes from enclosing expansions.
    /// Parse the stored bytes as the logical path grammar without native
    /// separator conversion or name normalization. During the walk, apply
    /// the same exact-name and native-limit rules as for the original operand.
    ///
    /// The returned expression is only the target segment. Walk it before
    /// resuming the suffix, retaining its directory requirement and keeping
    /// the boundary so missing target components cannot be created. Enforce
    /// the directory requirement of the original operand at its own end.
    ///
    /// The expanded length counts the relative base address (if any), target
    /// components and pending components, joined by single separators. It
    /// excludes the leading and trailing slash, and counts dot components as
    /// written before traversal. Each selected address is also length checked.
    pub fn prepare_lookup(
        &self,
        containing_directory: &StoragePath,
        pending: &[PathComponent],
    ) -> Result<PathExpr, Failure> {
        let too_long = Failure::Rejected(Rejection::PathTooLong);
        if self.0.len() > MAX_PATH_EXPR_BYTES {
            return Err(too_long);
        }
        let unrepresentable = Failure::Backend(BackendFailure::UnrepresentableLinkTarget);
        let text = std::str::from_utf8(&self.0).map_err(|_| unrepresentable)?;
        let target = PathExpr::parse(text).map_err(|error| match error {
            PathExprError::TooLong | PathExprError::InvalidName(NameError::TooLong) => too_long,
            PathExprError::Empty | PathExprError::InvalidName(_) => unrepresentable,
        })?;

        let mut length = match target.start() {
            LookupStart::InstanceRoot => 0,
            LookupStart::BaseDirectory => containing_directory.as_str().len(),
        };
        for component in target.components().iter().chain(pending) {
            length += usize::from(length != 0) + component.encoded_len();
            if length > MAX_PATH_BYTES {
                return Err(too_long);
            }
        }
        Ok(target)
    }
}

/// The intermediate-link policy and expansion budget for one operation.
/// Use one value for all walks in that operation, including rename's two
/// operands. Following a cycle exhausts the same budget as any long chain.
#[derive(Debug, Default)]
pub struct SymlinkTraversal {
    intermediate: IntermediateSymlinks,
    followed: u8,
}

impl SymlinkTraversal {
    pub fn new(intermediate: IntermediateSymlinks) -> Self {
        Self {
            intermediate,
            followed: 0,
        }
    }

    /// Decide what to do before reading a link's target. `pending` contains
    /// all unvisited components from the effective walk. A directory assertion
    /// such as a trailing slash is not a component. Apply the same decision
    /// after expansion, including to links introduced by the target segment.
    pub fn encounter(
        &mut self,
        lookup: PathLookup<'_>,
        pending: &[PathComponent],
    ) -> Result<SymlinkAction, Rejection> {
        let action = if pending.is_empty() {
            lookup.final_symlink()
        } else {
            match self.intermediate {
                IntermediateSymlinks::FollowWithinInstance => SymlinkAction::FollowWithinInstance,
                IntermediateSymlinks::Reject => return Err(Rejection::SymlinkNotAllowed),
            }
        };
        if action == SymlinkAction::FollowWithinInstance {
            if self.followed == MAX_SYMLINK_EXPANSIONS {
                return Err(Rejection::TooManySymlinks);
            }
            self.followed += 1;
        }
        Ok(action)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExistingTarget {
    Any,
    Absent,
    Directory,
    RegularFile,
    Symlink,
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
/// The execution port binds the instance and working directory before walking.
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
    pub fn check_address(self, address: &StoragePath) -> Result<(), Rejection> {
        if self.protect_root && address.is_root() {
            return Err(Rejection::RootProtected);
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
    ) -> Result<(), Rejection> {
        match (self.missing, name) {
            (MissingPolicy::Final | MissingPolicy::Recursive, MissingName::OperandFinal)
            | (MissingPolicy::Recursive, MissingName::OperandIntermediate) => {
                self.check_address(address)
            }
            _ => Err(Rejection::NotFound),
        }
    }

    /// Check an observed target after applying the final-link policy.
    /// Operations that accept a missing target must handle its absence
    /// according to their own mode.
    pub fn check_existing_target(
        self,
        address: &StoragePath,
        entry_type: EntryType,
    ) -> Result<(), Rejection> {
        self.check_address(address)?;
        self.check_directory(entry_type == EntryType::Directory)?;
        match self.existing {
            ExistingTarget::Absent => Err(Rejection::AlreadyExists),
            ExistingTarget::RegularFile => match entry_type {
                EntryType::File { .. } => Ok(()),
                EntryType::Directory => Err(Rejection::IsDirectory),
                EntryType::Symlink | EntryType::Other => Err(Rejection::NotRegularFile),
            },
            ExistingTarget::Symlink if entry_type != EntryType::Symlink => {
                Err(Rejection::NotSymlink)
            }
            ExistingTarget::NonDirectory if entry_type == EntryType::Directory => {
                Err(Rejection::IsDirectory)
            }
            ExistingTarget::Any
            | ExistingTarget::Directory
            | ExistingTarget::Symlink
            | ExistingTarget::NonDirectory => Ok(()),
        }
    }

    pub(super) fn check_missing_target_shape(
        self,
        address: &StoragePath,
        is_directory: bool,
    ) -> Result<(), Rejection> {
        self.check_missing_name(address, MissingName::OperandFinal)?;
        self.check_directory(is_directory)
    }

    fn check_directory(self, is_directory: bool) -> Result<(), Rejection> {
        if self.existing == ExistingTarget::Directory && !is_directory {
            return Err(Rejection::NotDirectory);
        }
        self.path.check_directory_requirement(is_directory)
    }
}
