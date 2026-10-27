use crate::{
    ConfigError, ConfigErrorKind, ConfigSection, ConfigWarning, SectionInfo,
    declaration::SharedValue,
};
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
    pub(crate) sections: Vec<SectionInfo>,
    pub(crate) warnings: Vec<ConfigWarning>,
}

impl LoadedConfig {
    /// The section declarations selected and successfully loaded for this snapshot.
    pub fn sections(&self) -> &[SectionInfo] {
        &self.sections
    }

    /// Nonfatal load conditions, retained independently of later file changes.
    pub fn warnings(&self) -> &[ConfigWarning] {
        &self.warnings
    }

    pub fn space(&self) -> &str {
        &self.space
    }

    /// Return the absolute source path; paths inside configuration values are untouched.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Share an already validated instance without reading, decoding or validating again.
    ///
    /// Only types derived with `Config` are accepted. A section outside this
    /// snapshot's selected space returns `NotRegistered`; `ConfigValue` and
    /// ordinary types are rejected at compile time.
    ///
    /// ```compile_fail
    /// fn read(config: &ah_config::LoadedConfig) {
    ///     let _ = config.get::<String>();
    /// }
    /// ```
    pub fn get<T: ConfigSection>(&self) -> Result<Arc<T>, ConfigError> {
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
                .with_section(T::KEY)
                .with_space(&self.space)
            })?
            .clone()
            .downcast::<T>()
            .map_err(|_| {
                ConfigError::message(ConfigErrorKind::Internal, "snapshot type mismatch")
                    .with_path(&self.path)
                    .with_section(T::KEY)
                    .with_space(&self.space)
            })
    }
}

impl fmt::Debug for LoadedConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedConfig")
            .field("space", &self.space)
            .field("sections", &self.values.len())
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}
