//! Guest application failures; serialization happens only at the dispatcher boundary.
use ah_vfs_storage::StorageError;
use vfs_plugin_protocol as p;
#[derive(Debug)]
pub enum Failure {
    Storage(StorageError),
    Control(p::Failure),
}
impl Failure {
    pub fn storage_error(&self) -> Option<&StorageError> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}
impl From<StorageError> for Failure {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}
impl From<p::Failure> for Failure {
    fn from(error: p::Failure) -> Self {
        Self::Control(error)
    }
}
impl From<p::CodecError> for Failure {
    fn from(error: p::CodecError) -> Self {
        Self::Control(error.into())
    }
}
impl From<Failure> for p::Failure {
    fn from(error: Failure) -> Self {
        match error {
            Failure::Control(error) => error,
            Failure::Storage(error) => match p::encode_storage_error(&error) {
                Ok(storage) => Self {
                    storage: Some(storage),
                    ..Self::new(p::FailureCode::Storage)
                },
                Err(_) => Self::new(p::FailureCode::ProtocolViolation),
            },
        }
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(error) => error.fmt(f),
            Self::Control(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Storage(error) => error,
            Self::Control(error) => error,
        })
    }
}
