use thiserror::Error;

/// Errors from namespace validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum NamespaceError {
    #[error("virtual path must be absolute and use canonical `/` separators")]
    InvalidVirtualPath,
    #[error("virtual path is {length} bytes, exceeding the {max}-byte limit")]
    VirtualPathTooLong { length: usize, max: usize },
    #[error(
        "entry name must be non-empty, must not be `.` or `..`, and must not contain separators or control characters"
    )]
    InvalidEntryName,
    #[error("entry name is {length} bytes, exceeding the {max}-byte limit")]
    EntryNameTooLong { length: usize, max: usize },
}
