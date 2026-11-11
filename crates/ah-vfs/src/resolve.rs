//! Resolve operands against a single backend operation's pinned entries.
//!
//! The backend observes one name at a time without following links. This
//! module owns navigation, traversal policy and the link expansion budget.
//!
//! # Path operands and lookup
//! Before any lookup or side effect, the execution port captures the selected
//! instance generation and its root directory identity. If an operand is
//! relative, it also captures the session's working-directory identity in that
//! instance. This is a directory binding, rather than a saved path to reopen or
//! the process working directory. A working directory is unnecessary for an
//! all-absolute request. Rename resolves the source first and uses the same
//! bindings and link budget for both operands. An invalidated generation or a
//! starting directory that no longer belongs to the bound namespace fails as
//! [`BackendFailure::Stale`]. Traversal denied by the confinement policy fails
//! as [`Rejection::AccessDenied`]. An adapter unable to enforce the binding or the
//! required lookup rules rejects the operation as [`Rejection::Unsupported`].
//!
//! [`PathExpr`] removes repeated separators but retains navigation components
//! and the trailing directory requirement. Start at the bound root for `/`, or
//! at the bound working directory for a relative operand. Walk left to right.
//! Every nonfinal component must yield a searchable directory. `.` retains the
//! current directory and `..` selects its actual parent within the instance.
//! At the bound root, `..` retains the root. For example, `missing/../a` fails
//! at `missing` in an ordinary lookup. A non-directory prefix fails as
//! [`Rejection::NotDirectory`] and denied search as [`Rejection::AccessDenied`].
//! Apply a trailing slash at the end of its segment. It requires a directory
//! and does not change how the final link is handled.
//!
//! The executor uses one [`SymlinkTraversal`] for the operation. A link is
//! intermediate if any component remains in the effective walk, including
//! suffixes waiting after a nested expansion. Otherwise the request's final
//! policy applies. A preserved link is selected directly, so `readlink link/`
//! fails as `NotDirectory` when `link` is a symbolic link. A followed link
//! consumes one expansion before reading its target. The 41st follow fails as
//! [`Rejection::TooManySymlinks`], including when the walk is cyclic.
//!
//! ReadLink returns a [`LinkTarget`] with the exact stored bytes. To follow it,
//! use [`prepare_link_target`] to parse and check the target and pending
//! suffix against the length bound. Walk the returned segment before resuming
//! the suffix. Absolute targets use the same instance root and relative targets
//! use the link's containing directory. Reapply the link policy on every
//! encounter. A target segment's own trailing slash must be satisfied even if
//! the original operand has no trailing slash. Use
//! [`PathExpr::check_directory_requirement`] when completing that segment.
//! ReadLink does not validate or normalize the stored text. On follow, a byte
//! sequence without a valid logical path representation fails as
//! [`BackendFailure::UnrepresentableLinkTarget`]. Native path syntax, such as
//! a drive prefix or backslash separators, is never rewritten. Logical length
//! violations fail as [`Rejection::PathTooLong`], including when joining a
//! component to a selected address would exceed [`MAX_PATH_BYTES`]. Once parsed,
//! target components obey the same exact-name and native-limit rules as any
//! other operand.
//!
//! Keep target segments distinct from original operand components and use
//! [`PathLookup::check_missing_name`] at an absent name. An absent name in a
//! followed target fails as `NotFound`. In the original operand, only
//! recursive directory creation may create missing intermediate names. At a
//! missing final name, apply the request's missing-target rule and, for write
//! and rename, its destination validation. Never create through a dangling
//! link. A preserved dangling link remains an existing entry for operations
//! such as ReadLink, Remove and Rename.
//!
//! [`PathLookup`] supplies pure checks on the selected address and kind. The
//! adapter must apply them to the same bound entry or parent-and-name slot used
//! by the operation, using handles, transactions or equivalent protection. At
//! staged publication, preserve the bound parent and verify the expected
//! destination identity or absence under the declared mutation guarantee.
//! A detected replacement before effects is [`Rejection::Conflict`]. Account separately for uncertain effects
//! after dispatch, including partial recursive creation. A [`StoragePath`] is
//! an address, and [`StoragePath::to_rooted_expr`] starts a fresh lookup from
//! the root. Neither supplies a durable entry binding across operations.
//! [`RenameRequest::decide_destination`](ah_vfs_storage::RenameRequest::decide_destination) distinguishes an unchanged same-slot
//! replacement from a move. Only a distinct directory destination must be empty.
//!
//! ```
//! use ah_vfs_storage::{ListRequest, ListTarget, LookupStart, PageSize, PathExpr, SymlinkAction};
//! let request = ListRequest::new(
//!     ListTarget::Path(PathExpr::parse("../docs//archive/")?),
//!     PageSize::DEFAULT,
//! );
//! let lookup = request.lookup().ok_or("initial lookup")?;
//! assert_eq!(lookup.path().start(), LookupStart::BaseDirectory);
//! assert_eq!(lookup.final_symlink(), SymlinkAction::FollowWithinInstance);
//! assert!(lookup.requires_directory());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! For example, with working directory `/work/sub` and `/work/link` pointing to
//! `../a/`, `../link//../file` visits `/work`, `/a`, then `/` before selecting
//! `/file`. The target's `../` starts at `/work`, not `/work/sub`. At `/`, a
//! further `..` stays at `/`. If `/a` is a file, the target segment fails with
//! `NotDirectory` before resuming the original operand.
//!
//! ```
//! use ah_vfs::resolve::prepare_link_target;
//! use ah_vfs_storage::{LinkTarget, LookupStart, PathComponent, PathExpr, Rejection, StoragePath};
//! let parent = StoragePath::parse("work")?;
//! let suffix = PathExpr::parse("../file")?;
//! let target = prepare_link_target(&LinkTarget::new(b"../a/".to_vec()), &parent, suffix.components())?;
//! assert_eq!(target.start(), LookupStart::BaseDirectory);
//! assert_eq!(target.components().first(), Some(&PathComponent::ParentDirectory));
//! assert_eq!(target.check_directory_requirement(false), Err(Rejection::NotDirectory));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use ah_vfs_storage::{
    BackendFailure, Entry, EntryName, EntryType, Failure, IntermediateSymlinks, LinkTarget,
    LookupStart, MAX_PATH_BYTES, MAX_PATH_EXPR_BYTES, MissingName, NameError, Operation,
    PathComponent, PathExpr, PathExprError, PathLookup, Rejection, StorageError, StoragePath,
    SymlinkAction,
};

/// Maximum number of link expansions in one operation, across both operands
/// of a rename and any links encountered inside other links' targets.
pub const MAX_SYMLINK_EXPANSIONS: u8 = 40;

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
pub fn prepare_link_target(
    link: &LinkTarget,
    containing_directory: &StoragePath,
    pending: &[PathComponent],
) -> Result<PathExpr, Failure> {
    let too_long = Failure::Rejected(Rejection::PathTooLong);
    if link.as_bytes().len() > MAX_PATH_EXPR_BYTES {
        return Err(too_long);
    }
    let unrepresentable = Failure::Backend(BackendFailure::UnrepresentableLinkTarget);
    let text = std::str::from_utf8(link.as_bytes()).map_err(|_| unrepresentable)?;
    let target = PathExpr::parse(text).map_err(|error| match error {
        PathExprError::TooLong | PathExprError::InvalidName(NameError::TooLong) => too_long,
        _ => unrepresentable,
    })?;

    let mut length = match target.start() {
        LookupStart::InstanceRoot => 0,
        LookupStart::BaseDirectory => containing_directory.as_str().len(),
    };
    for component in target.components().iter().chain(pending) {
        let bytes = match component {
            PathComponent::Name(name) => name.as_str().len(),
            PathComponent::CurrentDirectory => 1,
            PathComponent::ParentDirectory => 2,
        };
        length += usize::from(length != 0) + bytes;
        if length > MAX_PATH_BYTES {
            return Err(too_long);
        }
    }
    Ok(target)
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

/// The backend primitives for one operation. Bindings come from this backend
/// and may retain native handles. The walker only inspects their metadata.
pub trait LookupPort {
    type Binding: Clone;

    fn entry(binding: &Self::Binding) -> &Entry;

    fn observe_child(
        &mut self,
        parent: &Self::Binding,
        address: StoragePath,
        name: &EntryName,
    ) -> Result<Self::Binding, StorageError>;

    fn read_bound_link(&self, entry: &Self::Binding) -> Result<LinkTarget, StorageError>;

    fn create_child(
        &mut self,
        parent: &Self::Binding,
        address: StoragePath,
        name: &EntryName,
    ) -> Result<Self::Binding, StorageError>;
}

/// One operand walk budget. Rename uses the same walker for both operands.
pub struct Resolver {
    operation: Operation,
    links: SymlinkTraversal,
}

/// A resolved slot. A missing final name is returned only when the operation's
/// lookup policy admits it. Backends still bind and verify the parent at execution.
pub enum Resolution<B> {
    Existing(B),
    Missing(StoragePath),
}

impl<B> Resolution<B> {
    pub fn require_entry(self, operation: Operation) -> Result<B, StorageError> {
        match self {
            Self::Existing(binding) => Ok(binding),
            Self::Missing(_) => Err(StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::NotFound),
                None,
            )),
        }
    }
}

impl Resolver {
    pub fn new(operation: Operation, links: IntermediateSymlinks) -> Self {
        Self {
            operation,
            links: SymlinkTraversal::new(links),
        }
    }

    /// The backend supplies the root and, for a relative operand, the chain
    /// of pinned directories to the starting directory.
    pub fn walk<P: LookupPort>(
        &mut self,
        backend: &mut P,
        lookup: PathLookup<'_>,
        mut directories: Vec<P::Binding>,
    ) -> Result<Resolution<P::Binding>, StorageError> {
        self.segment(backend, lookup, lookup.path(), &mut directories, &[], false)
    }

    /// Resolve the parent of a final name, keeping that name in the effective
    /// suffix so links in the parent operand use the intermediate policy.
    pub fn parent<P: LookupPort>(
        &mut self,
        backend: &mut P,
        lookup: PathLookup<'_>,
        mut directories: Vec<P::Binding>,
    ) -> Result<(P::Binding, EntryName), StorageError> {
        let (operand, name) = lookup
            .path()
            .split_final_name()
            .ok_or_else(|| self.reject(Rejection::RootProtected))?;
        let parent = self
            .segment(
                backend,
                lookup,
                &operand,
                &mut directories,
                &[PathComponent::Name(name.clone())],
                false,
            )?
            .require_entry(self.operation)?;
        Ok((parent, name.clone()))
    }

    fn segment<P: LookupPort>(
        &mut self,
        backend: &mut P,
        lookup: PathLookup<'_>,
        expression: &PathExpr,
        directories: &mut Vec<P::Binding>,
        suffix: &[PathComponent],
        in_link: bool,
    ) -> Result<Resolution<P::Binding>, StorageError> {
        if expression.start() == LookupStart::InstanceRoot {
            directories.truncate(1);
        }
        let mut terminal = None;
        for (index, component) in expression.components().iter().enumerate() {
            let parent = directories.last().ok_or_else(|| self.stale())?;
            match component {
                PathComponent::CurrentDirectory => {}
                PathComponent::ParentDirectory if directories.len() > 1 => {
                    directories.pop();
                }
                PathComponent::ParentDirectory => {}
                PathComponent::Name(name) => {
                    let parent_address = P::entry(parent).path();
                    let address = parent_address
                        .join(name)
                        .map_err(|_| self.reject(Rejection::PathTooLong))?;
                    let remaining = &expression.components()[index + 1..];
                    let mut selected = match backend.observe_child(parent, address.clone(), name) {
                        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => {
                            let missing = if in_link {
                                MissingName::LinkTarget
                            } else if remaining.is_empty() && suffix.is_empty() {
                                MissingName::OperandFinal
                            } else {
                                MissingName::OperandIntermediate
                            };
                            match lookup.check_missing_name(&address, missing) {
                                Ok(()) => {}
                                Err(Rejection::NotFound) => return Err(error),
                                Err(reason) => return Err(self.reject(reason)),
                            }
                            if self.operation != Operation::CreateDirectory {
                                return Ok(Resolution::Missing(address));
                            }
                            backend.create_child(parent, address, name)?
                        }
                        result => result?,
                    };
                    if P::entry(&selected).entry_type() == EntryType::Symlink {
                        let pending: Vec<_> = remaining.iter().chain(suffix).cloned().collect();
                        if self
                            .links
                            .encounter(lookup, &pending)
                            .map_err(|reason| self.reject(reason))?
                            == SymlinkAction::FollowWithinInstance
                        {
                            let link = backend.read_bound_link(&selected)?;
                            let target = prepare_link_target(&link, parent_address, &pending)
                                .map_err(|reason| self.failure(reason))?;
                            selected = self
                                .segment(backend, lookup, &target, directories, &pending, true)?
                                .require_entry(self.operation)?;
                            if P::entry(&selected).entry_type() == EntryType::Directory {
                                // The expanded walk already left this directory on the stack.
                                continue;
                            }
                        }
                    }
                    if P::entry(&selected).entry_type() == EntryType::Directory {
                        directories.push(selected);
                    } else if remaining.is_empty() && suffix.is_empty() {
                        terminal = Some(selected);
                    } else {
                        return Err(self.reject(Rejection::NotDirectory));
                    }
                }
            }
        }
        let selected = terminal
            .or_else(|| directories.last().cloned())
            .ok_or_else(|| self.stale())?;
        expression
            .check_directory_requirement(P::entry(&selected).entry_type() == EntryType::Directory)
            .map_err(|reason| self.reject(reason))?;
        Ok(Resolution::Existing(selected))
    }

    fn reject(&self, reason: Rejection) -> StorageError {
        self.failure(Failure::Rejected(reason))
    }

    fn stale(&self) -> StorageError {
        self.failure(Failure::Backend(BackendFailure::Stale))
    }

    fn failure(&self, failure: Failure) -> StorageError {
        StorageError::without_effects(self.operation, failure, None)
    }
}
