use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum UserError {
    #[error("username must be 3-64 characters and contain only letters, numbers, '.', '_' or '-'")]
    InvalidUsername,
    #[error("credential hash must not be empty")]
    InvalidCredentialHash,
    #[error("permission must be read, write, or full")]
    InvalidPermission,
    #[error("password must contain at least 10 characters")]
    WeakPassword,
}
