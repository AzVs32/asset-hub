use serde::{Deserialize, Serialize};

use super::DEFAULT_SQLITE_MAX_CONNECTIONS;

/// 数据存储配置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    /// 当前启用的数据库后端。
    pub backend: DatabaseBackend,
    /// SQLite 后端专属配置。
    pub sqlite: SqliteDatabaseConfig,
}

/// 可用的数据库后端。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseBackend {
    #[default]
    Sqlite,
}

/// SQLite 后端专属配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SqliteDatabaseConfig {
    /// SQLite 连接池最大连接数。
    pub max_connections: u32,
}

impl Default for SqliteDatabaseConfig {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_SQLITE_MAX_CONNECTIONS,
        }
    }
}
