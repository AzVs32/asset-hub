//! Typed TOML configuration with isolated spaces and validated snapshots.
//!
//! The default `derive` feature provides `Config` for registered sections and
//! `ConfigValue` for nested values. Both generate Serde `Serialize` and
//! `Deserialize` implementations; a direct Serde dependency is not required.
//!
//! Select declarations with [`Registry::for_space`], then load a required or
//! optional file. All selected sections are decoded and validated before a
//! [`LoadedConfig`] is returned. [`LoadedConfig::get`] shares an already loaded
//! value through [`std::sync::Arc`]; it never reads the file again.
//!
//! ```
//! # #[cfg(feature = "derive")]
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use ah_config::Registry;
//!
//! #[derive(ah_config::Config)]
//! #[config(key = "http")]
//! struct HttpConfig {
//!     #[config(default = 8080)]
//!     port: u16,
//! }
//!
//! # let directory = tempfile::tempdir()?;
//! # std::fs::create_dir(directory.path().join("config"))?;
//! # std::fs::write(directory.path().join("config/config.toml"), "[http]\nport = 9000\n")?;
//! let config = Registry::for_space("default")?
//! #     .with_base_dir(directory.path())
//!     .load_required("config/config.toml")?;
//! let http = config.get::<HttpConfig>()?;
//! println!("HTTP port: {}", http.port);
//! # assert_eq!(http.port, 9000);
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "derive"))]
//! # fn main() {}
//! ```
//!
//! Absolute file paths are used unchanged. Relative paths use an explicit
//! [`Registry::with_base_dir`], Cargo's runtime package directory, or the
//! executable's directory, in that order. Configuration path values are untouched.
//!
//! Struct defaults merge recursively; explicitly supplied enums and arrays
//! replace their inherited defaults. An invalid input never falls back to a
//! default. Each load produces an independent snapshot; file changes do not
//! update existing values. User-defined interior mutability remains possible.
//! Both derive macros generate `Default` when a type-level default is declared.
//! Without that declaration, a manual or derived `Default` is allowed but does
//! not implicitly default a registered section or an unmarked parent field.
//! Direct `Default` construction does not run configuration validation.
//!
//! [`ConfigError`] exposes optional file, section and space context. Its
//! [`std::error::Error::source`] chain retains parser, I/O and validation causes.
//! Attribute placement and generated implementations are documented on the
//! derive macros. The [usage guide and examples](https://github.com/AzVs32/asset-hub/tree/main/libs/ah-config)
//! cover complete configuration scenarios.

extern crate self as ah_config;

mod declaration;
mod error;
mod loaded;
mod merge;
mod registry;
mod source;

#[cfg(feature = "derive")]
#[doc(inline)]
pub use ah_config_macros::{Config, ConfigValue};
pub use error::{ConfigError, ConfigErrorKind};
pub use loaded::LoadedConfig;
pub use registry::Registry;

/// Implementation details used by the derive macro, not a manual registration API.
#[doc(hidden)]
pub mod __private {
    pub use crate::declaration::{DECLARATIONS, Declaration, Prepared, ValidationError, prepare};
    pub use crate::merge::{DefaultMerge, MergeProbe, PrepareDefaults};
    pub use figment::value::Value;
    pub use linkme;
    pub use serde;
}
