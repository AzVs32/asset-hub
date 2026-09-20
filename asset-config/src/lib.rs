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
/// Generic sections cannot be registered automatically because there is no
/// concrete type to register. Use `auto = false` and [`Registry::register`].
///
/// ```compile_fail
/// use asset_config::config;
///
/// #[config(key = "generic", auto = true)]
/// #[derive(Default)]
/// struct Generic<T> {
///     value: T,
/// }
/// ```
///
/// The `key` argument is required and must not be empty.
///
/// ```compile_fail
/// use asset_config::config;
///
/// #[config]
/// #[derive(Default)]
/// struct MissingKey;
/// ```
///
/// ```compile_fail
/// use asset_config::config;
///
/// #[config(key = "  ")]
/// #[derive(Default)]
/// struct EmptyKey;
/// ```
pub use asset_config_macros::config;

#[doc(hidden)]
pub mod __private {
    pub use crate::auto::{AUTO_REGISTRATIONS, AutoRegistration};
    pub use linkme;
    pub use serde;
}
