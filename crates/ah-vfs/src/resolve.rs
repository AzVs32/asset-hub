//! Resolve path operands using one backend operation's bindings.
//!
//! [`Resolver`] walks components in order through the no-follow primitives in
//! [`LookupPort`]. Absolute operands start at the backend root. For relative
//! operands, [`LookupContext`] supplies a base address resolved for this call.
//! A later call resolves that address again and can select a replacement
//! directory. Within one walk, `.` retains the directory and `..` moves up the
//! bound chain, stopping at the root. A trailing slash requires a directory
//! and leaves the final-link policy unchanged.
//!
//! [`SymlinkTraversal`] applies the link policy to the effective suffix and
//! tracks one expansion budget across the operation, including both operands
//! of a rename. [`prepare_link_target`] interprets a relative target from the
//! link's parent and an absolute target from the root. A missing name in an
//! expanded target fails the walk. Only recursive directory creation can
//! create missing intermediate names in the original operand. A later
//! failure after a successful creation reports an `Unknown` outcome.
//!
//! [`read_link`] resolves an operand in the VFS and reads the selected link's
//! exact stored bytes. [`bind_link`] retains the same observations for a
//! later read. The backend verifies the bound slot for the canonical request.
//! A mismatch in a live generation returns `Conflict`. An invalidated
//! scope returns `Stale`.
//!
//! ```
//! use ah_vfs::application::port::{LookupContext, Storage};
//! use ah_vfs::resolve::read_link;
//! use ah_vfs_storage::PathExpr;
//!
//! async fn read_alias(storage: &dyn Storage) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
//!     let operand = PathExpr::parse("/docs/alias")?;
//!     let target = read_link(storage, operand, LookupContext::default()).await?;
//!     Ok(target.into_bytes())
//! }
//! ```

use crate::application::port::{
    BoundLink, LookupContext, ObservedEntry, OperationLookup, Storage, StorageFuture,
};
use ah_vfs_storage::{
    AttributeAction, AttributeRequest, AttributeResult, BackendFailure, ContractViolation, Entry,
    EntryName, EntryType, Failure, IntermediateSymlinks, LinkTarget, LookupStart, MAX_PATH_BYTES,
    MAX_PATH_EXPR_BYTES, MissingName, MutationOutcome, NameError, Operation, PathComponent,
    PathExpr, PathExprError, PathLookup, ReadLinkRequest, Rejection, StatRequest, StorageError,
    StoragePath, SymlinkAction,
};
use std::{
    future::{Future, ready},
    pin::pin,
    task::{Context, Poll, Waker},
};

/// Resolve a link operand on the caller side and retain the operation scope.
pub async fn bind_link(
    storage: &dyn Storage,
    operand: PathExpr,
    context: LookupContext,
) -> Result<Box<dyn BoundLink>, StorageError> {
    let op = Operation::ReadLink;
    let (backend, entry) = resolve_existing(storage, op, operand, context).await?;
    ReadLinkRequest::new(entry.observed.entry().path().clone())
        .check_target(entry.observed.entry().entry_type())
        .map_err(|reason| lookup_error(op, Failure::Rejected(reason)))?;
    backend.verify().await?;
    let slot = entry
        .slot
        .ok_or_else(|| lookup_error(op, Failure::Contract(ContractViolation::InvalidResponse)))?;
    Ok(Box::new(ResolvedLink {
        backend,
        slot,
        entry: entry.observed,
    }))
}

/// Resolve an attribute operand and apply its action to the observed entry.
pub async fn attribute(
    storage: &dyn Storage,
    operand: PathExpr,
    context: LookupContext,
    action: AttributeAction,
) -> Result<AttributeResult, StorageError> {
    let op = action.operation();
    let expected = action.expectation();
    let (backend, entry) = resolve_existing(storage, op, operand, context).await?;
    let request = AttributeRequest::new(entry.observed.entry().path().clone(), action);
    let slot = entry.slot.as_ref().map(|slot| (&slot.parent, &slot.name));
    let result = backend
        .execute_attribute(slot, &entry.observed, request)
        .await?;
    expected
        .validate(&result)
        .map_err(|reason| StorageError::after_dispatch(op, Failure::Contract(reason), None))?;
    Ok(result)
}

async fn resolve_existing(
    storage: &dyn Storage,
    op: Operation,
    operand: PathExpr,
    context: LookupContext,
) -> Result<(Box<dyn OperationLookup>, HostEntry), StorageError> {
    let mut backend = storage.begin_lookup(op).await?;
    if backend.operation() != op {
        return Err(lookup_error(
            op,
            Failure::Contract(ContractViolation::InvalidResponse),
        ));
    }
    let root = backend.root().await?;
    if !root.entry().path().is_root() || root.entry().entry_type() != EntryType::Directory {
        return Err(lookup_error(
            op,
            Failure::Contract(ContractViolation::InvalidEntry),
        ));
    }
    let mut lookup = HostLookup(backend.as_mut());
    let mut directories = vec![HostEntry {
        observed: root,
        slot: None,
    }];
    if operand.start() == LookupStart::BaseDirectory {
        let stale = || lookup_error(op, Failure::Backend(BackendFailure::Stale));
        for name in context.working_directory.entry_names() {
            let parent = directories.last().ok_or_else(stale)?;
            let address = parent
                .observed
                .entry()
                .path()
                .join(&name)
                .map_err(|_| stale())?;
            let directory =
                lookup
                    .observe_child(parent, address, &name)
                    .await
                    .map_err(|error| match error.failure() {
                        Failure::Rejected(
                            Rejection::NotFound
                            | Rejection::NotDirectory
                            | Rejection::SymlinkNotAllowed,
                        ) => stale(),
                        _ => error,
                    })?;
            if directory.observed.entry().entry_type() != EntryType::Directory {
                return Err(stale());
            }
            directories.push(directory);
        }
    }
    let request = StatRequest::new(operand, SymlinkAction::Preserve);
    let entry = Resolver::new(op, context.intermediate_symlinks)
        .walk_async(&mut lookup, request.lookup(), directories)
        .await?
        .require_entry(op)?;
    Ok((backend, entry))
}

/// Resolve and read one link without transmitting an operand to the backend.
pub async fn read_link(
    storage: &dyn Storage,
    operand: PathExpr,
    context: LookupContext,
) -> Result<LinkTarget, StorageError> {
    let binding = bind_link(storage, operand, context).await?;
    binding
        .read(ReadLinkRequest::new(binding.path().clone()))
        .await
}

#[derive(Clone)]
struct HostEntry {
    observed: ObservedEntry,
    slot: Option<ObservedSlot>,
}

#[derive(Clone)]
struct ObservedSlot {
    parent: ObservedEntry,
    name: EntryName,
}

struct ResolvedLink {
    backend: Box<dyn OperationLookup>,
    slot: ObservedSlot,
    entry: ObservedEntry,
}

impl BoundLink for ResolvedLink {
    fn path(&self) -> &StoragePath {
        self.entry.entry().path()
    }

    fn read(&self, request: ReadLinkRequest) -> StorageFuture<'_, LinkTarget> {
        Box::pin(async move {
            if request.path() != self.path() {
                return Err(lookup_error(
                    Operation::ReadLink,
                    Failure::InvalidBinding,
                ));
            }
            self.backend
                .execute_read_link(&self.slot.parent, &self.slot.name, &self.entry, request)
                .await
        })
    }
}

struct HostLookup<'a>(&'a mut dyn OperationLookup);

impl LookupPort for HostLookup<'_> {
    type Binding = HostEntry;

    fn entry(binding: &HostEntry) -> &Entry {
        binding.observed.entry()
    }

    async fn observe_child(
        &mut self,
        parent: &HostEntry,
        address: StoragePath,
        name: &EntryName,
    ) -> Result<HostEntry, StorageError> {
        let observed = self.0.observe(&parent.observed, name).await?;
        if observed.entry().path() != &address {
            return Err(lookup_error(
                self.0.operation(),
                Failure::Contract(ContractViolation::InvalidEntry),
            ));
        }
        Ok(HostEntry {
            observed,
            slot: Some(ObservedSlot {
                parent: parent.observed.clone(),
                name: name.clone(),
            }),
        })
    }

    async fn read_bound_link(&self, entry: &HostEntry) -> Result<LinkTarget, StorageError> {
        self.0.read_link(&entry.observed).await
    }

    async fn create_child(
        &mut self,
        _parent: &HostEntry,
        _address: StoragePath,
        _name: &EntryName,
    ) -> Result<HostEntry, StorageError> {
        Err(lookup_error(
            self.0.operation(),
            Failure::Rejected(Rejection::Unsupported),
        ))
    }
}

fn lookup_error(operation: Operation, failure: Failure) -> StorageError {
    StorageError::without_effects(operation, failure, None)
}

fn complete_immediately<T>(
    operation: Operation,
    future: impl Future<Output = Result<T, StorageError>>,
) -> Result<T, StorageError> {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => result,
        Poll::Pending => {
            let failure = Failure::Contract(ContractViolation::InvalidResponse);
            Err(if operation.may_mutate() {
                StorageError::after_dispatch(operation, failure, None)
            } else {
                StorageError::without_effects(operation, failure, None)
            })
        }
    }
}

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
///
/// ```
/// use ah_vfs::resolve::prepare_link_target;
/// use ah_vfs_storage::{LinkTarget, LookupStart, PathComponent, PathExpr, Rejection, StoragePath};
/// let parent = StoragePath::parse("work")?;
/// let suffix = PathExpr::parse("../file")?;
/// let link = LinkTarget::new(b"../a/".to_vec());
/// let target = prepare_link_target(&link, &parent, suffix.components())?;
/// assert_eq!(target.start(), LookupStart::BaseDirectory);
/// assert_eq!(target.components().first(), Some(&PathComponent::ParentDirectory));
/// assert_eq!(target.check_directory_requirement(false), Err(Rejection::NotDirectory));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
pub trait LookupPort: Send + Sync {
    type Binding: Clone + Send + Sync;

    fn entry(binding: &Self::Binding) -> &Entry;

    fn observe_child(
        &mut self,
        parent: &Self::Binding,
        address: StoragePath,
        name: &EntryName,
    ) -> impl Future<Output = Result<Self::Binding, StorageError>> + Send;

    fn read_bound_link(
        &self,
        entry: &Self::Binding,
    ) -> impl Future<Output = Result<LinkTarget, StorageError>> + Send;

    /// Create one absent child. An occupied slot returns `AlreadyExists`.
    /// If creation may have taken effect but the binding cannot be returned,
    /// report the error with `StorageError::after_dispatch`.
    fn create_child(
        &mut self,
        parent: &Self::Binding,
        address: StoragePath,
        name: &EntryName,
    ) -> impl Future<Output = Result<Self::Binding, StorageError>> + Send;

    /// Verify the observed bindings before a directory creation operation
    /// returns. A backend holding the namespace lock may use the default.
    fn verify_observations(&self) -> impl Future<Output = Result<(), StorageError>> + Send {
        ready(Ok(()))
    }
}

/// One operation's operand walks, traversal budget and visible directory effects.
/// Rename uses the same resolver for both operands.
pub struct Resolver {
    operation: Operation,
    links: SymlinkTraversal,
    created_directories: usize,
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
            created_directories: 0,
        }
    }

    /// The backend supplies bindings for the current operation. For a relative
    /// operand, the chain runs from the root to the directory currently at the
    /// supplied base address. Use this method with synchronous primitives
    /// that complete on their first poll.
    pub fn walk<P: LookupPort>(
        &mut self,
        backend: &mut P,
        lookup: PathLookup<'_>,
        directories: Vec<P::Binding>,
    ) -> Result<Resolution<P::Binding>, StorageError> {
        complete_immediately(
            self.operation,
            self.walk_async(backend, lookup, directories),
        )
    }

    /// Walk the same rules while allowing backend observations to await I/O.
    pub async fn walk_async<P: LookupPort>(
        &mut self,
        backend: &mut P,
        lookup: PathLookup<'_>,
        mut directories: Vec<P::Binding>,
    ) -> Result<Resolution<P::Binding>, StorageError> {
        let result = async {
            let resolved = self
                .segment(backend, lookup, lookup.path(), &mut directories, &[], false)
                .await?;
            if self.operation != Operation::CreateDirectory {
                return Ok(resolved);
            }
            let binding = resolved.require_entry(self.operation)?;
            let entry = P::entry(&binding);
            // Creating the final slot satisfies a single-level request's
            // absent-target policy. A recursive request requires a directory.
            if self.created_directories == 0 || lookup.final_symlink() != SymlinkAction::Preserve {
                lookup
                    .check_existing_target(entry.path(), entry.entry_type())
                    .map_err(|reason| self.reject(reason))?;
            }
            backend.verify_observations().await?;
            Ok(Resolution::Existing(binding))
        }
        .await;
        result.map_err(|error| self.include_creation_effects(error))
    }

    /// Resolve the parent of a final name, keeping that name in the effective
    /// suffix so links in the parent operand use the intermediate policy.
    /// Use this method with synchronous primitives.
    pub fn parent<P: LookupPort>(
        &mut self,
        backend: &mut P,
        lookup: PathLookup<'_>,
        directories: Vec<P::Binding>,
    ) -> Result<(P::Binding, EntryName), StorageError> {
        complete_immediately(
            self.operation,
            self.parent_async(backend, lookup, directories),
        )
    }

    pub async fn parent_async<P: LookupPort>(
        &mut self,
        backend: &mut P,
        lookup: PathLookup<'_>,
        mut directories: Vec<P::Binding>,
    ) -> Result<(P::Binding, EntryName), StorageError> {
        let (operand, name) = lookup
            .path()
            .split_final_name()
            .ok_or_else(|| self.include_creation_effects(self.reject(Rejection::RootProtected)))?;
        let parent = self
            .segment(
                backend,
                lookup,
                &operand,
                &mut directories,
                &[PathComponent::Name(name.clone())],
                false,
            )
            .await
            .and_then(|resolved| resolved.require_entry(self.operation))
            .map_err(|error| self.include_creation_effects(error))?;
        Ok((parent, name.clone()))
    }

    async fn segment<P: LookupPort>(
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
                    let mut selected =
                        match backend.observe_child(parent, address.clone(), name).await {
                            Err(error)
                                if error.failure() == Failure::Rejected(Rejection::NotFound)
                                    && error.outcome() == MutationOutcome::Unchanged =>
                            {
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
                                match backend.create_child(parent, address.clone(), name).await {
                                    Ok(binding) => {
                                        self.created_directories += 1;
                                        binding
                                    }
                                    Err(error)
                                        if lookup.final_symlink()
                                            == SymlinkAction::FollowWithinInstance
                                            && error.failure()
                                                == Failure::Rejected(Rejection::AlreadyExists)
                                            && error.outcome() == MutationOutcome::Unchanged =>
                                    {
                                        // Recursive creation may use a directory created
                                        // by another operation without claiming its effect.
                                        backend.observe_child(parent, address, name).await?
                                    }
                                    Err(error) => return Err(error),
                                }
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
                            let link = backend.read_bound_link(&selected).await?;
                            let target = prepare_link_target(&link, parent_address, &pending)
                                .map_err(|reason| self.failure(reason))?;
                            selected = Box::pin(self.segment(
                                backend,
                                lookup,
                                &target,
                                directories,
                                &pending,
                                true,
                            ))
                            .await?
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

    fn include_creation_effects(&self, error: StorageError) -> StorageError {
        if self.created_directories > 0 && error.outcome() == MutationOutcome::Unchanged {
            StorageError::after_dispatch(self.operation, error.failure(), Some(Box::new(error)))
        } else {
            error
        }
    }
}
