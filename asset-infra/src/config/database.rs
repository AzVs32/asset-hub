//! 数据库适配器配置。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_SQLITE_MAX_CONNECTIONS: u32 = 5;
const SQLITE_DATABASE_RELATIVE_PATH: &str = ".asset-hub/asset-hub.sqlite";

/// 数据库适配器配置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    /// 要使用的数据库后端。
    pub backend: DatabaseBackend,
    /// SQLite 后端配置。
    pub sqlite: SqliteDatabaseConfig,
}

/// Asset Hub 支持的数据库后端。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseBackend {
    /// SQLite 数据库。
    #[default]
    Sqlite,
}

/// SQLite 后端配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SqliteDatabaseConfig {
    /// SQLite 连接池的最大连接数。
    pub max_connections: u32,
}

impl Default for SqliteDatabaseConfig {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_SQLITE_MAX_CONNECTIONS,
        }
    }
}

impl DatabaseConfig {
    /// 校验所选数据库后端的配置。
    pub fn validate(&self) -> Result<(), String> {
        match self.backend {
            DatabaseBackend::Sqlite if self.sqlite.max_connections == 0 => {
                return Err("database.sqlite.max_connections must be greater than 0".to_owned());
            }
            DatabaseBackend::Sqlite => {}
        }
        Ok(())
    }

    pub(crate) fn sqlite_path_in(&self, data_root: &Path) -> PathBuf {
        match self.backend {
            DatabaseBackend::Sqlite => data_root.join(SQLITE_DATABASE_RELATIVE_PATH),
        }
    }
}
