//! Live, bounded directory enumeration with validated requests and responses.
use super::{Entry, EntryName, PathError, StoragePath};
use bon::bon;
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

/// List immediate children in exact UTF-8 name order, strictly after the cursor.
/// This is live enumeration: concurrent changes may affect subsequent pages.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct ListRequest {
    #[getset(get = "pub")]
    directory: StoragePath,
    after: Option<EntryName>,
    #[getset(get_copy = "pub")]
    size: PageSize,
}
#[bon]
impl ListRequest {
    /// Validate the cursor's full address when completing the builder.
    #[builder(start_fn = builder, state_mod(vis = "pub"))]
    pub fn new(
        directory: StoragePath,
        after: Option<EntryName>,
        #[builder(default)] size: PageSize,
    ) -> Result<Self, PathError> {
        if let Some(name) = &after {
            directory.join(name)?;
        }
        Ok(Self {
            directory,
            after,
            size,
        })
    }
    pub fn cursor(&self) -> Option<&EntryName> {
        self.after.as_ref()
    }
}
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum DirectoryPageError {
    #[error("page exceeds the requested entry count")]
    TooManyEntries,
    #[error("page contains an entry outside the requested directory")]
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
/// A checked response to one request. Continuation is derived from the final
/// entry, so an arbitrary or mismatched cursor cannot enter a valid page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryPage {
    entries: Vec<Entry>,
    next: Option<EntryName>,
}
#[bon]
impl DirectoryPage {
    /// Producers must explicitly decide whether more entries remain. Omitting
    /// that decision must never silently turn a partial result into a final page.
    #[builder(start_fn = builder, state_mod(vis = "pub"))]
    pub fn new(
        request: &ListRequest,
        entries: Vec<Entry>,
        has_more: bool,
    ) -> Result<Self, DirectoryPageError> {
        if entries.len() > request.size.get() {
            return Err(DirectoryPageError::TooManyEntries);
        }
        let mut previous = request.after.clone();
        for entry in &entries {
            if entry.path().parent().as_ref() != Some(&request.directory) {
                return Err(DirectoryPageError::NotAChild);
            }
            // Every immediate child has a validated nonempty final component.
            let name = entry.path().name().ok_or(DirectoryPageError::NotAChild)?;
            if request.after.as_ref().is_some_and(|after| &name <= after) {
                return Err(DirectoryPageError::BeforeCursor);
            }
            if previous.as_ref().is_some_and(|previous| &name <= previous) {
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
        Ok(Self { entries, next })
    }
    /// Decode a boundary response without discarding its supplied continuation.
    /// Producers may use builder() to derive a cursor; decoders must validate the
    /// actual supplied cursor here, including its relationship to the final entry.
    /// The builder requires next(Some(name)) or next(None), never an implicit end.
    #[builder(finish_fn = build, state_mod(vis = "pub"))]
    pub fn from_parts(
        request: &ListRequest,
        entries: Vec<Entry>,
        #[builder(required)] next: Option<EntryName>,
    ) -> Result<Self, DirectoryPageError> {
        let page = Self::new(request, entries, next.is_some())?;
        if page.next != next {
            return Err(DirectoryPageError::MismatchedCursor);
        }
        Ok(page)
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn next_after(&self) -> Option<&EntryName> {
        self.next.as_ref()
    }
    pub fn into_parts(self) -> (Vec<Entry>, Option<EntryName>) {
        (self.entries, self.next)
    }
}
