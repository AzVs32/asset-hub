//! VFS configuration, automatically registered with `asset-config`.
//!
//! Relative paths are resolved against the process working directory.
//! Loading configuration does not create directories or open the database.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Supported metadata database backends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseKind {
    #[default]
    Sqlite,
}

/// Settings in the `[vfs]` configuration section.
#[asset_config::config(key = "vfs")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsConfig {
    pub database: DatabaseKind,
    /// Local directory mounted at the virtual root `/`.
    pub root_mount_path: PathBuf,
    /// Shared directory for configuration, plugins, and the SQLite database.
    pub config_dir: PathBuf,
}

impl Default for VfsConfig {
    fn default() -> Self {
        Self {
            database: DatabaseKind::Sqlite,
            root_mount_path: PathBuf::from("data"),
            config_dir: PathBuf::from("conf"),
        }
    }
}

impl VfsConfig {
    /// Returns the database path using the fixed, non-configurable filename.
    pub fn sqlite_path(&self) -> PathBuf {
        self.config_dir.join("vfs.db")
    }
}
