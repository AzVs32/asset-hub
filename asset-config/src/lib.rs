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

pub use asset_config_macros::config;

#[doc(hidden)]
pub mod __private {
    pub use crate::auto::{AUTO_REGISTRATIONS, AutoRegistration};
    pub use linkme;
    pub use serde;
}
