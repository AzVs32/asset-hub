use thiserror::Error;

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DomainError {
    #[error("virtual path must be absolute")]
    VPathNotAbsolute,
    #[error("virtual path must use `/` as its only separator")]
    VPathContainsBackslash,
    #[error("virtual path must not contain control characters")]
    VPathContainsControlCharacter,
    #[error("virtual path must not contain repeated `/` separators")]
    VPathContainsRepeatedSeparator,
    #[error("virtual path must not end with `/` unless it is the root path")]
    VPathHasTrailingSeparator,
    #[error("virtual path must not contain `.` segments")]
    VPathContainsDotSegment,
    #[error("virtual path must not contain `..` segments")]
    VPathContainsDotDotSegment,
    #[error("virtual path is {length} bytes, exceeding the {max}-byte limit")]
    VPathTooLong { length: usize, max: usize },
    #[error("virtual path segment must not be empty")]
    VPathSegmentEmpty,
    #[error("virtual path segment must not contain `/`")]
    VPathSegmentContainsSeparator,
    #[error("virtual path segment is {length} bytes, exceeding the {max}-byte limit")]
    VPathSegmentTooLong { length: usize, max: usize },
}
