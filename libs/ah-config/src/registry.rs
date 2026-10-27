use crate::{
    ConfigError, ConfigErrorKind, ConfigWarning, LoadedConfig, SectionInfo,
    declaration::{DECLARATIONS, Prepared},
    source::Source,
};
use figment::Figment;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt, io,
    path::{Component, Path, PathBuf},
};

/// A single-use loading plan containing declarations from exactly one space.
///
/// Selecting a space prepares only its defaults. Loading reads and validates once,
/// then returns an independent snapshot. There is no global configuration cache.
/// Relative file paths use Cargo's package directory during Cargo runs and tests,
/// or the executable's directory otherwise, unless a base is supplied.
pub struct Registry {
    space: String,
    sections: Vec<Prepared>,
    section_info: Vec<SectionInfo>,
    base_dir: Option<PathBuf>,
}

impl Registry {
    /// List all linked space names in sorted, deduplicated order.
    ///
    /// This does not read files or run default factories and validators.
    pub fn spaces() -> Vec<&'static str> {
        DECLARATIONS
            .iter()
            .flat_map(|declaration| declaration.spaces.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// List all linked declarations, sorted by key, type name and space memberships.
    ///
    /// Queries do not prepare defaults, validate values or reject conflicting keys.
    /// Conflicting declarations remain visible so they can be diagnosed.
    /// Only crates linked into the current program contribute declarations.
    pub fn declarations() -> Vec<SectionInfo> {
        let mut declarations: Vec<_> = DECLARATIONS
            .iter()
            .map(|declaration| declaration.info())
            .collect();
        declarations.sort_by(|left, right| {
            (left.key(), left.type_name(), left.spaces()).cmp(&(
                right.key(),
                right.type_name(),
                right.spaces(),
            ))
        });
        declarations
    }

    /// Inspect declarations selected for this loading plan without running factories again.
    pub fn sections(&self) -> &[SectionInfo] {
        &self.section_info
    }

    /// Collect linked declarations for `space`, check conflicts, and prepare defaults.
    ///
    /// Names are case-sensitive and contain no whitespace or control characters.
    /// Unknown spaces fail. Unselected declarations never run their factories.
    pub fn for_space(space: impl AsRef<str>) -> Result<Self, ConfigError> {
        let space = space.as_ref();
        if space.is_empty()
            || space
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control())
        {
            return Err(ConfigError::message(
                ConfigErrorKind::InvalidSpace,
                format!("invalid space {space:?}"),
            )
            .with_space(space));
        }
        let mut declarations: Vec<_> = DECLARATIONS
            .iter()
            .filter(|declaration| declaration.spaces.contains(&space))
            .collect();
        if declarations.is_empty() {
            return Err(ConfigError::message(
                ConfigErrorKind::UnknownSpace,
                format!("no linked declarations for space {space:?}"),
            )
            .with_available_spaces(Self::spaces())
            .with_space(space));
        }
        declarations.sort_unstable_by_key(|declaration| declaration.key);
        let mut types = HashSet::new();
        for (index, declaration) in declarations.iter().enumerate() {
            if let Some(previous) = declarations[..index]
                .iter()
                .find(|previous| declaration.key == previous.key)
            {
                return Err(ConfigError::message(
                    ConfigErrorKind::KeyConflict,
                    format!(
                        "conflicting keys {:?} and {:?} in space {space:?}",
                        previous.key, declaration.key
                    ),
                )
                .with_section(declaration.key)
                .with_space(space));
            }
            if !types.insert((declaration.type_id)()) {
                return Err(ConfigError::message(
                    ConfigErrorKind::TypeConflict,
                    format!(
                        "duplicate type for key {:?} in space {space:?}",
                        declaration.key
                    ),
                )
                .with_section(declaration.key)
                .with_space(space));
            }
        }
        let section_info = declarations
            .iter()
            .map(|declaration| declaration.info())
            .collect();
        let sections = declarations
            .into_iter()
            .map(|declaration| {
                (declaration.prepare)()
                    .map_err(|error| error.with_section(declaration.key).with_space(space))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            space: space.into(),
            sections,
            section_info,
            base_dir: None,
        })
    }

    /// Override the automatic base directory for relative file paths.
    ///
    /// The base must be absolute; invalid bases fail when loading a relative path.
    /// Absolute file paths bypass the base. This setting applies in every build
    /// profile and does not rewrite path values inside the configuration.
    pub fn with_base_dir(mut self, base_dir: impl AsRef<Path>) -> Self {
        self.base_dir = Some(base_dir.as_ref().to_path_buf());
        self
    }

    /// Read a TOML file; a missing file is an error.
    ///
    /// Absolute paths are used unchanged. Relative paths use [`Self::with_base_dir`],
    /// then the runtime `CARGO_MANIFEST_DIR` if set, otherwise the executable's
    /// directory. The working directory is never used. Bases must be absolute.
    pub fn load_required(self, path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
        self.load(path.as_ref(), true)
    }

    /// Allow a missing file and construct selected sections from declared defaults.
    ///
    /// Missing required fields still fail. Read, syntax, type and validation failures
    /// never fall back to defaults. Paths follow the same rules as [`Self::load_required`].
    /// A successful default-only load records [`ConfigWarning::MissingFile`];
    /// inspect the snapshot's warnings to emit it through your own logger.
    pub fn load_optional(self, path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
        self.load(path.as_ref(), false)
    }

    fn load(mut self, path: &Path, required: bool) -> Result<LoadedConfig, ConfigError> {
        let path = self.resolve_path(path).map_err(|error| {
            ConfigError::file(ConfigErrorKind::Read, path, error).with_space(&self.space)
        })?;
        let (file, missing) =
            Source::file(&path, required).map_err(|error| error.with_space(&self.space))?;
        let mut config = Figment::new();
        for section in &mut self.sections {
            let supplied = file
                .check_section(section.key)
                .map_err(|error| error.with_path(&path).with_space(&self.space))?;
            if !supplied && section.defaults.is_none() && section.replace {
                return Err(ConfigError::message(
                    ConfigErrorKind::MissingSection,
                    format!("missing section {:?} in {path:?}", section.key),
                )
                .with_path(&path)
                .with_section(section.key)
                .with_space(&self.space));
            }
            if !supplied && section.defaults.is_none() {
                config = config.merge(&section.empty);
            }
            if !(section.replace && supplied)
                && let Some(defaults) = &mut section.defaults
            {
                (section.prepare_defaults)(defaults, &file, section.key);
                config = config.merge(&*defaults);
            }
        }
        let config = config.merge(&file);
        let mut values = HashMap::with_capacity(self.sections.len());
        for section in self.sections {
            values.insert(
                section.type_id,
                (section.load)(&config, &path, file.contains(section.key))
                    .map_err(|error| error.with_space(&self.space))?,
            );
        }
        Ok(LoadedConfig {
            space: self.space,
            sections: self.section_info,
            warnings: if missing {
                vec![ConfigWarning::MissingFile { path: path.clone() }]
            } else {
                Vec::new()
            },
            path,
            values,
        })
    }

    fn resolve_path(&self, path: &Path) -> io::Result<PathBuf> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
        // Windows drive-relative and rooted paths depend on the current directory.
        if path.as_os_str().is_empty()
            || path.has_root()
            || matches!(path.components().next(), Some(Component::Prefix(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected an absolute path or a nonempty relative path",
            ));
        }
        let base = match self
            .base_dir
            .clone()
            .or_else(|| std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from))
        {
            Some(base) => base,
            None => std::env::current_exe()?
                .parent()
                .ok_or_else(|| io::Error::other("executable has no parent directory"))?
                .to_path_buf(),
        };
        if !base.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration base directory must be absolute",
            ));
        }
        Ok(base.join(path))
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("space", &self.space)
            .field("sections", &self.sections.len())
            .field("base_dir", &self.base_dir)
            .finish_non_exhaustive()
    }
}
