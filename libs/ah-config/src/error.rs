use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

type Cause = Box<dyn Error + Send + Sync>;

/// Stable failure categories; additional categories may be introduced later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConfigErrorKind {
    InvalidSpace,
    UnknownSpace,
    KeyConflict,
    TypeConflict,
    Defaults,
    MissingFile,
    Read,
    Parse,
    MissingSection,
    Deserialize,
    Validation,
    NotRegistered,
    Internal,
}

impl fmt::Display for ConfigErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSpace => "invalid configuration space",
            Self::UnknownSpace => "configuration space has no declarations",
            Self::KeyConflict => "configuration section keys conflict",
            Self::TypeConflict => "configuration type is declared more than once",
            Self::Defaults => "invalid configuration defaults",
            Self::MissingFile => "required configuration file is missing",
            Self::Read => "cannot read configuration file",
            Self::Parse => "invalid TOML configuration file",
            Self::MissingSection => "required configuration section is missing",
            Self::Deserialize => "configuration section has an invalid type or field",
            Self::Validation => "configuration section failed validation",
            Self::NotRegistered => "configuration type is not registered in this snapshot",
            Self::Internal => "internal configuration error",
        })
    }
}

/// A typed failure with optional location metadata and an underlying error chain.
///
/// Display and Debug omit configuration values and underlying causes. Source chains
/// retain file, section and original parser or validator errors for diagnostics.
#[derive(thiserror::Error)]
#[error("{kind}")]
pub struct ConfigError {
    kind: ConfigErrorKind,
    path: Option<PathBuf>,
    section: Option<String>,
    space: Option<String>,
    #[source]
    source: Cause,
}

impl ConfigError {
    pub fn kind(&self) -> ConfigErrorKind {
        self.kind
    }

    /// Return the source file path, when known; it is absolute after path resolution.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Return the affected registered section key, when identifiable.
    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    /// Return the requested configuration space, when known.
    pub fn space(&self) -> Option<&str> {
        self.space.as_deref()
    }

    pub(crate) fn message(kind: ConfigErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            path: None,
            section: None,
            space: None,
            source: Box::new(Context {
                detail: detail.into(),
                source: None,
            }),
        }
    }

    pub(crate) fn caused(
        kind: ConfigErrorKind,
        detail: impl Into<String>,
        source: impl Into<Cause>,
    ) -> Self {
        Self {
            kind,
            path: None,
            section: None,
            space: None,
            source: Box::new(Context {
                detail: detail.into(),
                source: Some(source.into()),
            }),
        }
    }

    pub(crate) fn file(kind: ConfigErrorKind, path: &Path, source: impl Into<Cause>) -> Self {
        Self::caused(kind, format!("configuration file {path:?}"), source).with_path(path)
    }

    pub(crate) fn for_section(
        kind: ConfigErrorKind,
        path: &Path,
        key: &str,
        source: impl Into<Cause>,
    ) -> Self {
        Self::caused(
            kind,
            format!("configuration section {key:?} in {path:?}"),
            source,
        )
        .with_path(path)
        .with_section(key)
    }

    pub(crate) fn with_path(mut self, path: &Path) -> Self {
        self.path = Some(path.to_path_buf());
        self
    }

    pub(crate) fn with_section(mut self, section: &str) -> Self {
        self.section = Some(section.into());
        self
    }

    pub(crate) fn with_space(mut self, space: &str) -> Self {
        self.space = Some(space.into());
        self
    }
}

impl fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{detail}")]
struct Context {
    detail: String,
    #[source]
    source: Option<Cause>,
}
