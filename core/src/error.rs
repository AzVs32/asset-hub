use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error(transparent)]
    Storage(#[from] StorageError),
}

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {

}
