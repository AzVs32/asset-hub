//! Application startup configuration, registered with `asset-config`.
//!
//! Local root directories and metadata database selection belong to this
//! runtime. The VFS core only receives driver paths and repository interfaces.
//! Relative paths are resolved against the process working directory.
//! Loading configuration does not create directories or open the database.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Supported metadata database backends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum DatabaseKind {
    #[default]
    Sqlite,
}

/// Settings in the `[asset]` configuration section.
#[asset_config::config(key = "asset")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AssetConfig {
    pub database: DatabaseKind,
    /// Local directory mounted at the virtual root `/`.
    pub root_mount_path: PathBuf,
    /// Shared directory for configuration, plugins, and the SQLite database.
    pub config_dir: PathBuf,
}

#[cfg(test)]
mod tests;

impl Default for AssetConfig {
    fn default() -> Self {
        Self {
            database: DatabaseKind::Sqlite,
            root_mount_path: PathBuf::from("data"),
            config_dir: PathBuf::from("conf"),
        }
    }
}

impl AssetConfig {
    /// Returns the database path using the fixed, non-configurable filename.
    pub fn sqlite_path(&self) -> PathBuf {
        self.config_dir.join("vfs.db")
    }
}
