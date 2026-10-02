use std::path::Path;

use crate::{ConfigError, LoadedConfig, Registry};

#[doc(hidden)]
pub struct AutoRegistration {
    pub key: &'static str,
    pub register: fn(&mut Registry) -> Result<(), ConfigError>,
}

#[linkme::distributed_slice]
pub static AUTO_REGISTRATIONS: [AutoRegistration];

/// Loads concrete sections registered by `#[config]` or `register!`.
///
/// Non-generic sections marked `auto = false` and generic definitions without
/// an explicit `register!` declaration are excluded. Use `Registry` to load a
/// selected set of sections instead.
/// Each section is deserialized and validated once, then stored as a shared instance.
/// Every call reads the file independently and returns a fresh [`LoadedConfig`],
/// even when the path is unchanged. There is no global configuration cache.
pub fn load(path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
    let mut registrations: Vec<_> = AUTO_REGISTRATIONS.iter().collect();
    registrations.sort_unstable_by_key(|registration| registration.key);

    let mut registry = Registry::default();
    for registration in registrations {
        (registration.register)(&mut registry)?;
    }
    registry.load(path)
}
