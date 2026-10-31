//! Local-driver database configuration, independent of the application database.

use ah_config::{ConfigError, LoadedConfig};
use ah_vfs::config::DriverConfigNamespace;
use std::path::PathBuf;

/// Database settings registered by LocalDriver as `[vfs.driver.local]`.
///
/// Omitting the section selects SQLite at `vfs-driver-local.sqlite`. A present
/// section requires `backend` and accepts only fields for that backend.
#[derive(ah_config::Config)]
#[config(
    parent = DriverConfigNamespace,
    key = "local",
    tag = "backend",
    default = Self::Sqlite,
    deny_unknown_fields,
    validate = validate_database,
)]
pub enum LocalDriverConfig {
    /// An isolated SQLite memory database without a backing file.
    Memory {},
    /// A SQLite file, relative to the selected configuration file's parent.
    Sqlite {
        #[config(default = "vfs-driver-local.sqlite".into())]
        path: PathBuf,
    },
    /// A PostgreSQL URL; runtime support is not implemented yet.
    Postgres { url: String },
}

impl LocalDriverConfig {
    /// Resolve paths against the loaded configuration file without creating files.
    ///
    /// The snapshot retains the original path. No runtime crate is required to
    /// load or validate this driver's declaration.
    pub fn from_loaded(loaded: &LoadedConfig) -> Result<Self, ConfigError> {
        let config = loaded.get::<Self>()?;
        Ok(match &*config {
            Self::Memory {} => Self::Memory {},
            Self::Sqlite { path } => {
                let path = if path.is_absolute() {
                    path.clone()
                } else {
                    let mut dir = loaded.path().to_path_buf();
                    dir.pop();
                    dir.join(path)
                };
                Self::Sqlite { path }
            }
            Self::Postgres { url } => Self::Postgres { url: url.clone() },
        })
    }
}

fn validate_database(config: &LocalDriverConfig) -> Result<(), &'static str> {
    match config {
        LocalDriverConfig::Sqlite { path } if path.as_os_str().is_empty() => {
            Err("LocalDriver SQLite path cannot be empty")
        }
        LocalDriverConfig::Postgres { url } if url.trim().is_empty() => {
            Err("LocalDriver Postgres URL cannot be empty")
        }
        _ => Ok(()),
    }
}
