use thiserror::Error;

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DomainError {
    #[error("virtual path must be absolute")]
    VPathNotAbsolute,
    #[error("virtual path must not contain `..` segments")]
    VPathContainsDotDotSegment,
}
