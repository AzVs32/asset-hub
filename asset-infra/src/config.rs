use ::config::{Config, File, FileFormat};
use asset_core::CoreError;
use asset_core::port::ResourceActionDefinition;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 默认配置文件名。
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
/// 默认 SQLite 数据库文件路径。
pub const DEFAULT_SQLITE_PATH: &str = "data/asset-hub.sqlite";
/// 默认 Fs 对象存储根目录。
pub const DEFAULT_FS_ROOT: &str = "data/blob";
/// 默认 SQLite 连接池最大连接数。
pub const DEFAULT_SQLITE_MAX_CONNECTIONS: u32 = 5;

/// 基础设施配置。
///
/// 该配置由 `config` crate 加载，并允许配置文件为空。空文件或缺失字段都会使用默认值：
/// - SQLite 数据库文件：`data/asset-hub.sqlite`
/// - Fs 对象存储根目录：`data/blob`
/// - SQLite 最大连接数：`5`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetInfraConfig {
    /// 数据存储配置。
    pub database: DatabaseConfig,
    /// 对象存储配置。
    pub blob: BlobConfig,
    /// 资源类型注册表配置。
    pub kind: KindRegistryConfig,
}

impl AssetInfraConfig {
    /// 从 TOML 字符串解析配置。
    ///
    /// 空字符串会被视为默认配置。缺失的字段会逐层填充默认值。
    pub fn from_config_str(value: &str) -> Result<Self, CoreError> {
        build_config()
            .add_source(File::from_str(value, FileFormat::Toml))
            .build()
            .and_then(Config::try_deserialize)
            .map_err(config_error)
    }

    /// 从配置文件读取配置。
    ///
    /// 文件存在但内容为空时，会返回默认配置。文件不存在时返回配置错误。
    pub fn from_config_file(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        Self::load_config_file(path, true)
    }

    /// 从可选配置文件读取配置。
    ///
    /// 文件不存在时不会报错，会直接使用默认配置。该方法用于默认 `config.toml` 这类
    /// “有则读取，无则默认”的启动场景。
    pub fn from_optional_config_file(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        Self::load_config_file(path, false)
    }

    /// 从默认配置文件 `config.toml` 读取配置。
    ///
    /// 当前工作目录下没有 `config.toml` 时，会使用默认配置。
    pub fn from_default_config_file() -> Result<Self, CoreError> {
        Self::from_optional_config_file(DEFAULT_CONFIG_FILE)
    }

    fn load_config_file(path: impl AsRef<Path>, required: bool) -> Result<Self, CoreError> {
        build_config()
            .add_source(
                File::from(path.as_ref())
                    .format(FileFormat::Toml)
                    .required(required),
            )
            .build()
            .and_then(Config::try_deserialize)
            .map_err(config_error)
    }

    /// 归一化配置。
    ///
    /// 当前主要处理路径：SQLite 文件路径和 Fs root 可以在配置中写相对路径，归一化后会
    /// 基于当前工作目录转换成绝对路径。
    pub fn normalized(mut self) -> Result<Self, CoreError> {
        self.database.sqlite_path = normalize_path(&self.database.sqlite_path)?;
        self.blob.fs_root = normalize_path(&self.blob.fs_root)?;
        self.kind.plugin_manifest_dirs = self
            .kind
            .plugin_manifest_dirs
            .iter()
            .map(|path| normalize_path(path))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self)
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

/// 资源类型注册表配置。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KindRegistryConfig {
    /// 由系统配置声明的资源类型。
    pub definitions: Vec<ResourceKindConfig>,
    /// 插件 manifest 目录。目录中的 JSON manifest 会在启动时加载。
    pub plugin_manifest_dirs: Vec<PathBuf>,
}

/// 配置文件中的资源类型定义。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceKindConfig {
    /// 资源类型值，例如 `asset:image`。
    pub kind: String,
    /// 展示名称；为空时使用 `kind`。
    pub label: Option<String>,
    /// 默认 kind metadata schema id。
    pub schema_id: Option<String>,
    /// kind metadata JSON schema。
    pub metadata_schema: Option<Value>,
    /// 是否支持对象内容。
    pub supports_content: bool,
    /// kind 支持的动作，例如 `read`、`thumbnail`。
    pub actions: Vec<ResourceActionDefinition>,
}

impl Default for ResourceKindConfig {
    fn default() -> Self {
        Self {
            kind: String::new(),
            label: None,
            schema_id: None,
            metadata_schema: None,
            supports_content: true,
            actions: Vec::new(),
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

fn build_config() -> ::config::ConfigBuilder<::config::builder::DefaultState> {
    Config::builder()
        .set_default("database.sqlite_path", DEFAULT_SQLITE_PATH)
        .expect("default sqlite path should be a valid config value")
        .set_default(
            "database.max_connections",
            i64::from(DEFAULT_SQLITE_MAX_CONNECTIONS),
        )
        .expect("default sqlite max connections should be a valid config value")
        .set_default("blob.fs_root", DEFAULT_FS_ROOT)
        .expect("default blob root should be a valid config value")
}

fn config_error(error: ::config::ConfigError) -> CoreError {
    CoreError::configuration(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_uses_defaults() {
        let config = AssetInfraConfig::from_config_str("").unwrap();

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
    fn partial_config_keeps_missing_defaults() {
        let config = AssetInfraConfig::from_config_str(
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
    fn kind_config_accepts_static_definitions_and_plugin_dirs() {
        let config = AssetInfraConfig::from_config_str(
            r#"
            [kind]
            plugin_manifest_dirs = ["plugins"]

            [[kind.definitions]]
            kind = "doc:note"
            label = "Note"
            schema_id = "doc:note@1"
            supports_content = false
            metadata_schema = { type = "object" }
            "#,
        )
        .unwrap();

        assert_eq!(config.kind.plugin_manifest_dirs, [PathBuf::from("plugins")]);
        assert_eq!(config.kind.definitions.len(), 1);
        assert_eq!(config.kind.definitions[0].kind, "doc:note");
        assert_eq!(config.kind.definitions[0].label.as_deref(), Some("Note"));
        assert_eq!(
            config.kind.definitions[0].schema_id.as_deref(),
            Some("doc:note@1")
        );
        assert!(!config.kind.definitions[0].supports_content);
        assert_eq!(
            config.kind.definitions[0].metadata_schema.as_ref().unwrap()["type"],
            "object"
        );
    }

    #[test]
    fn optional_missing_config_file_uses_defaults() {
        let config = AssetInfraConfig::from_optional_config_file(
            std::env::temp_dir().join("asset-hub-missing-config.toml"),
        )
        .unwrap();

        assert_eq!(
            config.database.sqlite_path,
            PathBuf::from(DEFAULT_SQLITE_PATH)
        );
        assert_eq!(config.blob.fs_root, PathBuf::from(DEFAULT_FS_ROOT));
    }

    #[test]
    fn normalized_config_turns_relative_paths_into_absolute_paths() {
        let config = AssetInfraConfig::default().normalized().unwrap();

        assert!(config.database.sqlite_path.is_absolute());
        assert!(config.blob.fs_root.is_absolute());
    }

    #[test]
    fn normalized_config_turns_plugin_manifest_dirs_into_absolute_paths() {
        let config = AssetInfraConfig {
            kind: KindRegistryConfig {
                plugin_manifest_dirs: vec![PathBuf::from("plugins")],
                ..KindRegistryConfig::default()
            },
            ..AssetInfraConfig::default()
        }
        .normalized()
        .unwrap();

        assert!(config.kind.plugin_manifest_dirs[0].is_absolute());
    }
}
