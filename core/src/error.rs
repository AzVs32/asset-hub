use thiserror::Error;

use crate::path::error::DomainError;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CoreError {
    #[error(transparent)]
    Path(#[from] DomainError),
}
