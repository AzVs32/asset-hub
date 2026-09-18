use thiserror::Error;

use crate::mount::error::DomainError;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CoreError {
    #[error(transparent)]
    Mount(#[from] DomainError),
}
