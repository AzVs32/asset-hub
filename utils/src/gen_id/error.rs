use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid identifier")]
pub struct ParseIdError;
