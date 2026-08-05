use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum UserError {
    #[error("username must be 3-64 characters and contain only letters, numbers, '.', '_' or '-'")]
    InvalidUsername,
    #[error("credential hash must not be empty")]
    InvalidCredentialHash,
    #[error("user updated timestamp cannot precede creation")]
    InvalidTimestamps,
    #[error("password must contain at least 4 characters")]
    WeakPassword,
}
