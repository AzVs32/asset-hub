//! Live, bounded directory enumeration with validated requests and responses.
use super::{
    BackendFailure, Entry, EntryName, PathExpr, PathLookup, StoragePath, SymlinkAction,
    lookup::ExistingTarget,
};
use getset::{CopyGetters, Getters};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum PageSizeError {
    #[error("page size must be positive")]
    Zero,
    #[error("page size exceeds the maximum")]
    TooLarge,
}
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PageSize(usize);
impl PageSize {
    pub const MAX: usize = 1000;
    pub const DEFAULT: Self = Self(100);
    pub fn new(value: usize) -> Result<Self, PageSizeError> {
        if value == 0 {
            return Err(PageSizeError::Zero);
        }
        if value > Self::MAX {
            return Err(PageSizeError::TooLarge);
        }
        Ok(Self(value))
    }
    pub fn get(self) -> usize {
        self.0
    }
}
impl Default for PageSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A backend-supplied directory incarnation identity. Within an execution
/// boundary, equality must identify the same directory in the same instance
/// generation. A replaced directory or a different instance generation must
/// have a different identity, even at the same address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectoryIdentity([u8; 16]);

impl DirectoryIdentity {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// The address and identity of a directory selected by the executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectorySelection {
    address: StoragePath,
    identity: DirectoryIdentity,
}

impl DirectorySelection {
    /// Supply an actual directory binding established by the executor, such as
    /// through lookup or a retained handle. The caller attests to the origin.
    /// Values copied from a page payload do not establish this binding.
    pub fn from_lookup(address: StoragePath, identity: DirectoryIdentity) -> Self {
        Self { address, identity }
    }

    pub fn address(&self) -> &StoragePath {
        &self.address
    }

    pub fn identity(&self) -> DirectoryIdentity {
        self.identity
    }
}

/// A continuation produced by a checked page. Its exclusive name and selected
/// directory travel together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryCursor {
    selection: DirectorySelection,
    after: EntryName,
}

impl DirectoryCursor {
    /// Restore a serialized checkpoint. Execution must independently verify the
    /// directory binding before using the name as a continuation.
    pub fn from_checkpoint(
        selection: DirectorySelection,
        after: EntryName,
    ) -> Result<Self, super::PathError> {
        selection.address().join(&after)?;
        Ok(Self { selection, after })
    }

    pub fn selection(&self) -> &DirectorySelection {
        &self.selection
    }

    pub fn after(&self) -> &EntryName {
        &self.after
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListTarget {
    /// Resolve a fresh operand under the operation's lookup context.
    Path(PathExpr),
    /// Resume at the selected address and directory identity in the cursor.
    Continue(DirectoryCursor),
}

/// List immediate children in exact UTF-8 name order. A continuation keeps the
/// selected address and identity. Enumeration is live, so directory contents
/// can change between pages.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct ListRequest {
    #[getset(get = "pub")]
    target: ListTarget,
    #[getset(get_copy = "pub")]
    size: PageSize,
}

impl ListRequest {
    pub fn new(target: ListTarget, size: PageSize) -> Self {
        Self { target, size }
    }

    /// Lookup for an initial request. A continuation uses its bound selection
    /// instead of the original operand or working directory.
    pub fn lookup(&self) -> Option<PathLookup<'_>> {
        match &self.target {
            ListTarget::Path(path) => Some(PathLookup::new(
                path,
                SymlinkAction::FollowWithinInstance,
                ExistingTarget::Directory,
            )),
            ListTarget::Continue(_) => None,
        }
    }

    /// Associate this request with the directory independently selected by the
    /// executor. For an initial request, establish the selection through this
    /// request's lookup. To resume, obtain the cursor's directory by a retained
    /// binding or independently reopen its rooted address and verify its
    /// identity. A changed address or identity is stale.
    pub fn bind_selection(
        &self,
        selection: DirectorySelection,
    ) -> Result<BoundListRequest<'_>, BackendFailure> {
        if let ListTarget::Continue(cursor) = &self.target
            && cursor.selection != selection
        {
            return Err(BackendFailure::Stale);
        }
        Ok(BoundListRequest {
            request: self,
            selection,
        })
    }
}

/// The request and its independently established directory binding.
/// Enumerate that binding and correlate the page response with this operation.
/// When decoding, keep this context outside the page payload.
#[derive(Debug)]
pub struct BoundListRequest<'a> {
    request: &'a ListRequest,
    selection: DirectorySelection,
}

impl BoundListRequest<'_> {
    pub fn request(&self) -> &ListRequest {
        self.request
    }

    pub fn selection(&self) -> &DirectorySelection {
        &self.selection
    }

    pub fn size(&self) -> PageSize {
        self.request.size
    }

    pub fn after(&self) -> Option<&EntryName> {
        match &self.request.target {
            ListTarget::Path(_) => None,
            ListTarget::Continue(cursor) => Some(cursor.after()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum DirectoryPageError {
    #[error("page directory disagrees with the bound directory")]
    UnexpectedDirectory,
    #[error("page exceeds the requested entry count")]
    TooManyEntries,
    #[error("page contains an entry outside the bound directory")]
    NotAChild,
    #[error("page entries must be strictly increasing")]
    NotOrdered,
    #[error("page contains an entry at or before the exclusive cursor")]
    BeforeCursor,
    #[error("an empty page cannot provide a continuation")]
    EmptyContinuation,
    #[error("continuation does not match the final returned entry")]
    MismatchedCursor,
}

/// A page checked against a bound request. Each continuation derives its name
/// from the final returned entry and retains the selected directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryPage {
    selection: DirectorySelection,
    entries: Vec<Entry>,
    next: Option<EntryName>,
}

impl DirectoryPage {
    /// Build a page from entries read through the bound directory. Producers
    /// use the same exact name mapping as lookup and explicitly decide whether
    /// more entries remain. This checks logical page structure. The producer
    /// establishes the native spelling of each entry.
    pub fn new(
        bound: &BoundListRequest<'_>,
        entries: Vec<Entry>,
        has_more: bool,
    ) -> Result<Self, DirectoryPageError> {
        if entries.len() > bound.size().get() {
            return Err(DirectoryPageError::TooManyEntries);
        }
        let after = bound.after().map(EntryName::as_str);
        let mut previous = None;
        for entry in &entries {
            let (parent, name) = entry
                .path()
                .split_name()
                .ok_or(DirectoryPageError::NotAChild)?;
            if parent != bound.selection.address.as_str() {
                return Err(DirectoryPageError::NotAChild);
            }
            if after.is_some_and(|after| name <= after) {
                return Err(DirectoryPageError::BeforeCursor);
            }
            if previous.is_some_and(|previous| name <= previous) {
                return Err(DirectoryPageError::NotOrdered);
            }
            previous = Some(name);
        }
        let next = if has_more {
            Some(
                entries
                    .last()
                    .and_then(|entry| entry.path().name())
                    .ok_or(DirectoryPageError::EmptyContinuation)?,
            )
        } else {
            None
        };
        Ok(Self {
            selection: bound.selection.clone(),
            entries,
            next,
        })
    }

    /// Decode a page against the binding already established for this request.
    /// Check both response claims, even for an empty page. The binding must
    /// originate outside the page payload. Decoders explicitly supply the
    /// response's next name or its end marker.
    pub fn from_parts(
        bound: &BoundListRequest<'_>,
        reported_address: &StoragePath,
        reported_identity: DirectoryIdentity,
        entries: Vec<Entry>,
        next: Option<EntryName>,
    ) -> Result<Self, DirectoryPageError> {
        if reported_address != bound.selection.address()
            || reported_identity != bound.selection.identity()
        {
            return Err(DirectoryPageError::UnexpectedDirectory);
        }
        let page = Self::new(bound, entries, next.is_some())?;
        if page.next != next {
            return Err(DirectoryPageError::MismatchedCursor);
        }
        Ok(page)
    }

    pub fn selection(&self) -> &DirectorySelection {
        &self.selection
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn continuation(&self) -> Option<DirectoryCursor> {
        let after = self.next.as_ref()?.clone();
        Some(DirectoryCursor {
            selection: self.selection.clone(),
            after,
        })
    }

    /// Take a checkpoint at an already validated entry, for example when a
    /// namespace overlay consumes only a prefix of this page.
    pub fn checkpoint(&self, index: usize) -> Option<DirectoryCursor> {
        Some(DirectoryCursor {
            selection: self.selection.clone(),
            after: self.entries.get(index)?.path().name()?,
        })
    }

    /// The validated fields for encoding a page response.
    pub fn into_parts(self) -> (DirectorySelection, Vec<Entry>, Option<EntryName>) {
        (self.selection, self.entries, self.next)
    }
}
