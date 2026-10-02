#![doc = include_str!("../README.md")]

extern crate self as asset_config;

mod auto;
mod error;
mod loaded;
mod registry;
mod section;

pub use auto::load;
pub use error::ConfigError;
pub use loaded::LoadedConfig;
pub use registry::Registry;
pub use section::ConfigSection;

/// Defines a configuration section and, by default, registers it for [`load`].
///
/// A section must implement [`Default`] so missing values can be filled in.
/// `validate = path` accepts a function returning `Result<(), E>`, where
/// `E: Into<Box<dyn std::error::Error + Send + Sync>>`; failures retain their source.
/// Conditional-compilation attributes (`cfg` and nested `cfg_attr`) apply to
/// both the generated implementation and automatic registration.
///
/// ```compile_fail,E0277
/// use asset_config::config;
///
/// #[config(key = "missing_default")]
/// struct MissingDefault {
///     value: bool,
/// }
/// ```
///
/// Generic definitions only generate the trait implementation. Use
/// [`register!`] to register a concrete instance for automatic loading.
/// `auto = false` excludes non-generic sections from automatic loading, for
/// tests or selected [`Registry`] instances.
///
/// ```
/// use asset_config::{config, register};
///
/// #[config(key = "generic")]
/// #[derive(Default)]
/// struct Generic<T> {
///     value: T,
/// }
/// register!(Generic<u32>);
/// let loaded = asset_config::load(std::env::temp_dir().join("generic-example.toml"))?;
/// assert_eq!(loaded.get::<Generic<u32>>()?.value, 0);
/// # Ok::<(), asset_config::ConfigError>(())
/// ```
///
/// The `key` argument is required. Its format is checked when the section is registered.
///
/// ```compile_fail
/// use asset_config::config;
///
/// #[config]
/// #[derive(Default)]
/// struct MissingKey;
/// ```
pub use asset_config_macros::config;

/// Registers an existing [`ConfigSection`] for automatic [`load`].
///
/// Supports concrete generic instances such as `register!(GenericConfig<u32>)`
/// and uses the type's [`ConfigSection::KEY`]. Place the declaration at module
/// scope. Repeated or overlapping keys are rejected during registration.
///
/// The type must already implement `ConfigSection`:
///
/// ```compile_fail
/// #[derive(Default)]
/// struct NotASection;
/// asset_config::register!(NotASection);
/// ```
pub use asset_config_macros::register;

#[doc(hidden)]
pub mod __private {
    pub use crate::auto::{AUTO_REGISTRATIONS, AutoRegistration};
    pub use linkme;
    pub use serde;
}
