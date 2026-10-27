use crate::{ConfigError, ConfigErrorKind, merge::DefaultMerge};
use figment::{
    Metadata, Profile, Provider,
    providers::{Format, Serialized, Toml},
    value::{Dict, Map, Value},
};
use serde::Serialize;
use std::path::Path;

/// Materialized provider data so files and defaults are evaluated only once.
pub(crate) struct Source {
    metadata: Metadata,
    data: Map<Profile, Dict>,
}

impl Source {
    pub(crate) fn defaults<T: Serialize>(key: &str, value: T) -> Result<Self, ConfigError> {
        let provider = Serialized::default(key, value);
        Ok(Self {
            metadata: provider.metadata(),
            data: provider.data().map_err(|error| {
                ConfigError::caused(
                    ConfigErrorKind::Defaults,
                    format!("defaults for section {key:?}"),
                    error,
                )
                .with_section(key)
            })?,
        })
    }

    pub(crate) fn file(path: &Path, required: bool) -> Result<(Self, bool), ConfigError> {
        let (text, missing) = match std::fs::read_to_string(path) {
            Ok(text) => (text, false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => {
                (String::new(), true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ConfigError::file(ConfigErrorKind::MissingFile, path, error));
            }
            Err(error) => return Err(ConfigError::file(ConfigErrorKind::Read, path, error)),
        };
        let data = Toml::string(&text)
            .data()
            .map_err(|error| ConfigError::file(ConfigErrorKind::Parse, path, error))?;
        Ok((
            Self {
                metadata: Metadata::from("TOML configuration", path),
                data,
            },
            missing,
        ))
    }

    pub(crate) fn contains(&self, key: &str) -> bool {
        self.value(key).is_some()
    }

    /// Distinguish absent sections from a non-table ancestor in the supplied file.
    pub(crate) fn check_section(&self, key: &str) -> Result<bool, ConfigError> {
        let Some(mut fields) = self.data.get(&Profile::Default) else {
            return Ok(false);
        };
        let mut segments = key.split('.').peekable();
        let mut ancestor = String::new();
        while let Some(segment) = segments.next() {
            let Some(value) = fields.get(segment) else {
                return Ok(false);
            };
            if segments.peek().is_none() {
                return Ok(true);
            }
            if !ancestor.is_empty() {
                ancestor.push('.');
            }
            ancestor.push_str(segment);
            fields = value.as_dict().ok_or_else(|| {
                ConfigError::message(
                    ConfigErrorKind::Deserialize,
                    format!("section {key:?} requires a table at {ancestor:?}"),
                )
                .with_section(key)
            })?;
        }
        Ok(false)
    }

    fn value(&self, key: &str) -> Option<&Value> {
        let root = self.data.get(&Profile::Default)?;
        let mut segments = key.split('.');
        let mut value = root.get(segments.next()?)?;
        for segment in segments {
            value = value.as_dict()?.get(segment)?;
        }
        Some(value)
    }

    /// Discard defaults at explicitly supplied enum boundaries, including nested ones.
    pub(crate) fn prepare_defaults<T: DefaultMerge>(&mut self, supplied: &Self, key: &str) {
        let Some(supplied) = supplied.value(key) else {
            return;
        };
        let Some(root) = self.data.get_mut(&Profile::Default) else {
            return;
        };
        let mut segments = key.split('.');
        let Some(mut defaults) = segments.next().and_then(|key| root.get_mut(key)) else {
            return;
        };
        for segment in segments {
            let Value::Dict(_, fields) = defaults else {
                return;
            };
            let Some(next) = fields.get_mut(segment) else {
                return;
            };
            defaults = next;
        }
        T::prepare_defaults(defaults, supplied);
    }
}

impl Provider for Source {
    fn metadata(&self) -> Metadata {
        self.metadata.clone()
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        Ok(self.data.clone())
    }
}
