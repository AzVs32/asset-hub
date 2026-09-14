use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum UtilsError {
    #[error(transparent)]
    ParseId(#[from] ParseIdError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid identifier")]
pub struct ParseIdError;
