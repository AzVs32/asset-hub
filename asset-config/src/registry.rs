use crate::auto::{self, ASSET_CONFIG_AUTO_REGISTRATIONS};
use crate::error::ConfigError;
use crate::loaded::{LoadedConfig, SharedValue};
use crate::section::ConfigSection;
use config::{Config, File, Value};
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

type SectionLoader = fn(&Config) -> Result<SharedValue, ConfigError>;

struct Registration {
    key: &'static str,
    type_id: TypeId,
    default: Value,
    load: SectionLoader,
}

/// Selects configuration sections for one independent load.
///
/// A new registry starts empty and ignores automatic declarations. Register
/// selected types with [`Self::register`], then consume it with [`Self::load`].
/// This is useful for tests, loading a subset of sections, and selecting concrete
/// generic instances. Create a new registry to read the file again.
/// [`Self::from_auto`] starts with automatic declarations and allows adding
/// selected types before loading.
///
/// ```
/// #[asset_config::config(key = "registry_example", auto = false)]
/// #[derive(Default)]
/// struct Settings { enabled: bool }
///
/// let mut registry = asset_config::Registry::new();
/// registry.register::<Settings>()?;
/// let configs = registry.load(std::env::temp_dir().join("registry-example.toml"))?;
/// let first = configs.get::<Settings>()?;
/// let second = configs.get::<Settings>()?;
/// assert!(std::sync::Arc::ptr_eq(&first, &second));
/// # Ok::<(), asset_config::ConfigError>(())
/// ```
#[derive(Default)]
pub struct Registry {
    registrations: Vec<Registration>,
    keys: HashSet<&'static str>,
}

impl Registry {
    /// Creates an empty registry without collecting automatic declarations.
    pub fn new() -> Self {
        Self::default()
    }

    /// Collects declarations made with `#[config]` and `register!`.
    ///
    /// Registers concrete sections in key order, checking their keys and recording
    /// serialized defaults without reading a file or running validators. The
    /// registry can be extended with [`Self::register`] before [`Self::load`].
    ///
    /// # Errors
    ///
    /// Returns registration errors described by [`Self::register`], including
    /// invalid or conflicting keys and failures to serialize default values.
    pub fn from_auto() -> Result<Self, ConfigError> {
        auto::collect(&ASSET_CONFIG_AUTO_REGISTRATIONS)
    }

    /// Registers a concrete configuration type and records its serialized defaults.
    ///
    /// Key checks happen immediately. `T::default()` and serialization run during
    /// registration; file reading, deserialization, and validation run in [`Self::load`].
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::InvalidKey`] for an invalid section path,
    /// [`ConfigError::KeyConflict`] for duplicate or overlapping paths, or
    /// [`ConfigError::Backend`] if the default value cannot be serialized.
    pub fn register<T>(&mut self) -> Result<&mut Self, ConfigError>
    where
        T: ConfigSection,
    {
        if !valid_key(T::KEY) {
            return Err(ConfigError::InvalidKey(T::KEY));
        }

        if let Some(existing) = self
            .keys
            .iter()
            .filter(|&&key| key == T::KEY || is_ancestor(key, T::KEY) || is_ancestor(T::KEY, key))
            .min()
            .copied()
        {
            return Err(ConfigError::KeyConflict {
                existing,
                incoming: T::KEY,
            });
        }

        let type_id = TypeId::of::<T>();
        let default = Config::try_from(&T::default())?.cache;
        let load: SectionLoader = |config| {
            let value = config.get::<T>(T::KEY)?;
            value.validate()?;
            Ok(Arc::new(value))
        };

        self.registrations.push(Registration {
            key: T::KEY,
            type_id,
            default,
            load,
        });
        self.keys.insert(T::KEY);
        Ok(self)
    }

    /// Loads, deserializes, and validates each registered section once.
    ///
    /// File values override `T::default()`. Returns shared instances only after
    /// every section succeeds; later reads do not deserialize or validate again.
    /// The registry is consumed. A missing file uses the registered defaults;
    /// each new load reads the file independently and creates a fresh [`LoadedConfig`].
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Backend`] for file, parsing, or deserialization
    /// failures, or [`ConfigError::Validation`] if a user validator rejects a section.
    /// Previously loaded instances are dropped when a later section fails.
    pub fn load(self, path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
        let mut builder = Config::builder();

        for registration in &self.registrations {
            builder = builder.set_default(registration.key, registration.default.clone())?
        }
        builder = builder.add_source(File::from(path.as_ref()).required(false));

        let inner = builder.build()?;

        let mut values = HashMap::with_capacity(self.registrations.len());
        for registration in self.registrations {
            values.insert(registration.type_id, (registration.load)(&inner)?);
        }
        Ok(LoadedConfig { values })
    }
}

fn valid_key(key: &str) -> bool {
    key.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    })
}

fn is_ancestor(parent: &str, child: &str) -> bool {
    child
        .strip_prefix(parent)
        .is_some_and(|suffix| suffix.starts_with('.'))
}
