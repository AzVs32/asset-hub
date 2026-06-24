use super::ResourceError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error(transparent)]
    Resource(#[from] ResourceError),
}
