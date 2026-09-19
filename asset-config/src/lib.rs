mod error;
mod loaded;
mod registry;
mod section;

pub use error::ConfigError;
pub use loaded::LoadedConfig;
pub use registry::Registry;
pub use section::ConfigSection;

pub use asset_config_macros::config;

#[doc(hidden)]
pub mod __private {
    pub use serde;
}
