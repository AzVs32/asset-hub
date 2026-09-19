use std::path::Path;

use crate::{ConfigError, LoadedConfig, Registry};

#[doc(hidden)]
pub struct AutoRegistration {
    pub key: &'static str,
    pub register: fn(&mut Registry) -> Result<(), ConfigError>,
}

#[linkme::distributed_slice]
pub static AUTO_REGISTRATIONS: [AutoRegistration];

/// Loads all configuration sections declared with `#[config]`.
///
/// Sections marked `auto = false` are excluded. Use `Registry` to load a
/// selected set of sections instead.
pub fn load(path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
    let mut registrations: Vec<_> = AUTO_REGISTRATIONS.iter().collect();
    registrations.sort_unstable_by_key(|registration| registration.key);

    let mut registry = Registry::default();
    for registration in registrations {
        (registration.register)(&mut registry)?;
    }
    registry.load(path)
}
