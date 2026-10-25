use crate::{ConfigError, ConfigErrorKind, declaration::SharedValue};
use std::{
    any::TypeId,
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

/// An immutable, independently loaded and validated configuration snapshot.
///
/// Repeated reads share instances within this snapshot. Separate loads never share
/// instances implicitly. Updating a file has no effect on an existing snapshot.
pub struct LoadedConfig {
    pub(crate) space: String,
    pub(crate) path: PathBuf,
    pub(crate) values: HashMap<TypeId, SharedValue>,
}

impl LoadedConfig {
    pub fn space(&self) -> &str {
        &self.space
    }

    /// Return the absolute source path; paths inside configuration values are untouched.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Share an already validated instance without reading, decoding or validating again.
    pub fn get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, ConfigError> {
        self.values
            .get(&TypeId::of::<T>())
            .ok_or_else(|| {
                ConfigError::message(
                    ConfigErrorKind::NotRegistered,
                    format!(
                        "type {} is absent from space {:?}",
                        std::any::type_name::<T>(),
                        self.space
                    ),
                )
                .with_path(&self.path)
                .with_space(&self.space)
            })?
            .clone()
            .downcast::<T>()
            .map_err(|_| {
                ConfigError::message(ConfigErrorKind::Internal, "snapshot type mismatch")
                    .with_path(&self.path)
                    .with_space(&self.space)
            })
    }
}

impl fmt::Debug for LoadedConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedConfig")
            .field("space", &self.space)
            .field("sections", &self.values.len())
            .finish_non_exhaustive()
    }
}
