use asset_core::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 默认 SQLite 数据库文件路径。
pub const DEFAULT_SQLITE_PATH: &str = "data/asset-hub.sqlite";
/// 默认 Fs 对象存储根目录。
pub const DEFAULT_FS_ROOT: &str = "data/blob";
/// 默认 SQLite 连接池最大连接数。
pub const DEFAULT_SQLITE_MAX_CONNECTIONS: u32 = 5;

/// 基础设施配置。
///
/// 该配置允许配置文件为空。空文件或缺失字段都会使用默认值：
/// - SQLite 数据库文件：`data/asset-hub.sqlite`
/// - Fs 对象存储根目录：`data/blob`
/// - SQLite 最大连接数：`5`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetInfraConfig {
    /// 数据存储配置。
    pub database: DatabaseConfig,
    /// 对象存储配置。
    pub blob: BlobConfig,
}

impl AssetInfraConfig {
    /// 从 TOML 字符串解析配置。
    ///
    /// 空字符串会被视为默认配置。缺失的字段会逐层填充默认值。
    pub fn from_toml_str(value: &str) -> Result<Self, CoreError> {
        toml::from_str(value).map_err(|error| CoreError::configuration(error.to_string()))
    }

    /// 从 TOML 文件读取配置。
    ///
    /// 如果文件存在但内容为空，会返回默认配置。文件不存在仍然返回错误，调用方如果希望
    /// “未提供配置文件”等价于默认配置，应直接使用 `AssetInfraConfig::default()`。
    pub fn from_toml_file(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        let value = std::fs::read_to_string(path)
            .map_err(|error| CoreError::configuration(error.to_string()))?;

        Self::from_toml_str(&value)
    }

    /// 归一化配置。
    ///
    /// 当前主要处理路径：SQLite 文件路径和 Fs root 可以在配置中写相对路径，归一化后会
    /// 基于当前工作目录转换成绝对路径。
    pub fn normalized(mut self) -> Result<Self, CoreError> {
        self.database.sqlite_path = normalize_path(&self.database.sqlite_path)?;
        self.blob.fs_root = normalize_path(&self.blob.fs_root)?;
        Ok(self)
    }
}

impl Default for AssetInfraConfig {
    fn default() -> Self {
        Self {
            database: DatabaseConfig::default(),
            blob: BlobConfig::default(),
        }
    }
}

/// 数据存储配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// SQLite 数据库文件路径。
    ///
    /// 可以是相对路径或绝对路径。相对路径会在初始化时按当前工作目录转换为绝对路径。
    pub sqlite_path: PathBuf,
    /// SQLite 连接池最大连接数。
    pub max_connections: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            sqlite_path: PathBuf::from(DEFAULT_SQLITE_PATH),
            max_connections: DEFAULT_SQLITE_MAX_CONNECTIONS,
        }
    }
}

/// 对象存储配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlobConfig {
    /// OpenDAL Fs backend 根目录。
    ///
    /// 可以是相对路径或绝对路径。相对路径会在初始化时按当前工作目录转换为绝对路径。
    pub fs_root: PathBuf,
}

impl Default for BlobConfig {
    fn default() -> Self {
        Self {
            fs_root: PathBuf::from(DEFAULT_FS_ROOT),
        }
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf, CoreError> {
    if path.as_os_str().is_empty() {
        return Err(CoreError::configuration("path must not be empty"));
    }

    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    std::env::current_dir()
        .map(|current_dir| current_dir.join(path))
        .map_err(|error| CoreError::configuration(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_uses_defaults() {
        let config = AssetInfraConfig::from_toml_str("").unwrap();

        assert_eq!(
            config.database.sqlite_path,
            PathBuf::from(DEFAULT_SQLITE_PATH)
        );
        assert_eq!(
            config.database.max_connections,
            DEFAULT_SQLITE_MAX_CONNECTIONS
        );
        assert_eq!(config.blob.fs_root, PathBuf::from(DEFAULT_FS_ROOT));
    }

    #[test]
    fn partial_toml_keeps_missing_defaults() {
        let config = AssetInfraConfig::from_toml_str(
            r#"
            [blob]
            fs_root = "tmp/blob"
            "#,
        )
        .unwrap();

        assert_eq!(
            config.database.sqlite_path,
            PathBuf::from(DEFAULT_SQLITE_PATH)
        );
        assert_eq!(config.blob.fs_root, PathBuf::from("tmp/blob"));
    }

    #[test]
    fn normalized_config_turns_relative_paths_into_absolute_paths() {
        let config = AssetInfraConfig::default().normalized().unwrap();

        assert!(config.database.sqlite_path.is_absolute());
        assert!(config.blob.fs_root.is_absolute());
    }
}
