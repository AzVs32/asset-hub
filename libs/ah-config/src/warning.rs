use std::{fmt, path::PathBuf};

/// A nonfatal condition from a successful configuration load.
///
/// The library does not print warnings. Applications can inspect
/// [`crate::LoadedConfig::warnings`] and route them to their own logger.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigWarning {
    /// An optional file was absent and the declared defaults passed validation.
    MissingFile { path: PathBuf },
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFile { path } => write!(
                f,
                "configuration file {path:?} was not found; using declared defaults"
            ),
        }
    }
}
