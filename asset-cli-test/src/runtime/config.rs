//! Application startup configuration, registered with `asset-config`.
//!
//! Local root directories and metadata database selection belong to this
//! runtime. The VFS core only receives driver paths and repository interfaces.
//! Storage directories must use absolute paths.
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
    /// Absolute local directory mounted at the virtual root `/`.
    pub root_mount_path: PathBuf,
    /// Absolute shared directory for configuration, plugins, and SQLite.
    pub config_dir: PathBuf,
}

#[cfg(test)]
mod tests;

impl Default for AssetConfig {
    fn default() -> Self {
        Self {
            database: DatabaseKind::Sqlite,
            root_mount_path: PathBuf::from("/asset-hub-data"),
            config_dir: PathBuf::from("/asset-hub-conf"),
        }
    }
}

impl AssetConfig {
    /// Returns the database path using the fixed, non-configurable filename.
    pub fn sqlite_path(&self) -> PathBuf {
        self.config_dir.join("asset.db")
    }
}
