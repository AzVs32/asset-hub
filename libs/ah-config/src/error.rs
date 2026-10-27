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
/// Display and Debug include available location metadata, but omit configuration
/// values and underlying causes. Source chains retain original errors for diagnostics.
pub struct ConfigError {
    kind: ConfigErrorKind,
    path: Option<PathBuf>,
    section: Option<String>,
    space: Option<String>,
    field_path: Option<String>,
    source: Box<Context>,
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

    /// Return the full TOML field path for decoding errors, when available.
    ///
    /// Validation functions return section-level errors without a field path.
    /// TOML syntax errors and arbitrary custom decoding errors may also lack one.
    pub fn field_path(&self) -> Option<&str> {
        self.field_path.as_deref()
    }

    /// Linked space names suggested for `UnknownSpace`; empty for other errors.
    pub fn available_spaces(&self) -> &[&'static str] {
        &self.source.available_spaces
    }

    pub(crate) fn message(kind: ConfigErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            path: None,
            section: None,
            space: None,
            field_path: None,
            source: Box::new(Context {
                detail: detail.into(),
                available_spaces: Vec::new(),
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
            field_path: None,
            source: Box::new(Context {
                detail: detail.into(),
                available_spaces: Vec::new(),
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

    pub(crate) fn decode(
        kind: ConfigErrorKind,
        path: &Path,
        key: &str,
        error: figment::Error,
    ) -> Self {
        // extract_inner appends the section path after the relative error path.
        let section: Vec<_> = key.split('.').map(str::to_owned).collect();
        let relative = error
            .path
            .strip_suffix(section.as_slice())
            .unwrap_or(&error.path);
        let mut fields = relative.to_vec();
        if let figment::error::Kind::MissingField(field) = &error.kind {
            fields.push(field.to_string());
        }
        let field_path = (!fields.is_empty()).then(|| format!("{key}.{}", fields.join(".")));
        let mut result = Self::for_section(kind, path, key, error);
        result.field_path = field_path;
        result
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

    pub(crate) fn with_available_spaces(mut self, spaces: Vec<&'static str>) -> Self {
        self.source.available_spaces = spaces;
        self
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(path) = &self.path {
            write!(f, " [file: {path:?}]")?;
        }
        if let Some(section) = &self.section {
            write!(f, " [section: {section:?}]")?;
        }
        if let Some(field) = &self.field_path {
            write!(f, " [field: {field:?}]")?;
        }
        if let Some(space) = &self.space {
            write!(f, " [space: {space:?}]")?;
        }
        if self.kind == ConfigErrorKind::UnknownSpace {
            write!(f, " [available spaces: {:?}]", self.available_spaces())?;
        }
        Ok(())
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ConfigError");
        debug
            .field("kind", &self.kind)
            .field("path", &self.path)
            .field("section", &self.section)
            .field("field_path", &self.field_path)
            .field("space", &self.space);
        if self.kind == ConfigErrorKind::UnknownSpace {
            debug.field("available_spaces", &self.source.available_spaces);
        }
        debug.finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{detail}")]
struct Context {
    detail: String,
    available_spaces: Vec<&'static str>,
    #[source]
    source: Option<Cause>,
}
