use ::config::{Config, File, FileFormat};
use asset_core::CoreError;
use asset_core::port::{ResourceActionDefinition, ResourceContentMatcher};
use asset_core::service::PluginExecutionPolicy;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 默认配置文件名。
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
/// 默认 SQLite 数据库文件路径。
pub const DEFAULT_SQLITE_PATH: &str = "data/.asset-hub/asset-hub.sqlite";
/// 默认 Fs 对象存储根目录。
pub const DEFAULT_FS_ROOT: &str = "data";
/// 默认 SQLite 连接池最大连接数。
pub const DEFAULT_SQLITE_MAX_CONNECTIONS: u32 = 5;
pub const DEFAULT_PLUGIN_MAX_CONTENT_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_PLUGIN_MAX_INLINE_CONTENT_BYTES: u64 = 4 * 1024 * 1024;
pub const DEFAULT_PLUGIN_MAX_CONTENT_READ_BYTES: u64 = 4 * 1024 * 1024;
pub const DEFAULT_PLUGIN_MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_PLUGIN_MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_PLUGIN_MAX_CONCURRENT_CALLS: usize = 8;
pub const DEFAULT_PLUGIN_MEMORY_MAX_PAGES: u32 = 4096;
pub const DEFAULT_PLUGIN_TIMEOUT_SECONDS: u64 = 20;

/// 基础设施配置。
///
/// 该配置由 `config` crate 加载，并允许配置文件为空。空文件或缺失字段都会使用默认值：
/// - SQLite 数据库文件：`data/.asset-hub/asset-hub.sqlite`
/// - Fs 对象存储根目录：`data`
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
    /// 插件执行预算和宿主批准的外部权限。
    pub plugin: PluginHostConfig,
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
        self.kind.plugin_manifests = self
            .kind
            .plugin_manifests
            .iter()
            .map(|path| normalize_path(path))
            .collect::<Result<Vec<_>, _>>()?;
        self.plugin.normalize_and_validate()?;
        Ok(self)
    }
}

/// 插件宿主策略。Manifest 只能请求权限，最终授权必须同时出现在这里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginHostConfig {
    pub max_content_bytes: u64,
    pub max_inline_content_bytes: u64,
    pub max_content_read_bytes: u64,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_concurrent_calls: usize,
    pub memory_max_pages: u32,
    pub timeout_seconds: u64,
    pub grants: PluginPermissionGrants,
}

impl Default for PluginHostConfig {
    fn default() -> Self {
        Self {
            max_content_bytes: DEFAULT_PLUGIN_MAX_CONTENT_BYTES,
            max_inline_content_bytes: DEFAULT_PLUGIN_MAX_INLINE_CONTENT_BYTES,
            max_content_read_bytes: DEFAULT_PLUGIN_MAX_CONTENT_READ_BYTES,
            max_input_bytes: DEFAULT_PLUGIN_MAX_INPUT_BYTES,
            max_output_bytes: DEFAULT_PLUGIN_MAX_OUTPUT_BYTES,
            max_concurrent_calls: DEFAULT_PLUGIN_MAX_CONCURRENT_CALLS,
            memory_max_pages: DEFAULT_PLUGIN_MEMORY_MAX_PAGES,
            timeout_seconds: DEFAULT_PLUGIN_TIMEOUT_SECONDS,
            grants: PluginPermissionGrants::default(),
        }
    }
}

impl PluginHostConfig {
    pub fn execution_policy(&self) -> Result<PluginExecutionPolicy, CoreError> {
        PluginExecutionPolicy::new(
            self.max_content_bytes,
            self.max_inline_content_bytes,
            self.max_content_read_bytes,
            self.max_input_bytes,
            self.max_output_bytes,
            self.max_concurrent_calls,
            self.memory_max_pages,
            self.timeout_seconds,
        )
        .map_err(|error| CoreError::configuration(error.to_string()))
    }

    fn normalize_and_validate(&mut self) -> Result<(), CoreError> {
        self.execution_policy()?;
        for host in &self.grants.network_hosts {
            if host.is_empty() || host.trim() != host || host.contains('*') {
                return Err(CoreError::configuration(format!(
                    "plugin.grants.network_hosts contains invalid host `{host}`"
                )));
            }
        }
        self.grants.filesystem_read = self
            .grants
            .filesystem_read
            .iter()
            .map(|path| normalize_permission_grant(path))
            .collect::<Result<_, _>>()?;
        self.grants.filesystem_write = self
            .grants
            .filesystem_write
            .iter()
            .map(|path| normalize_permission_grant(path))
            .collect::<Result<_, _>>()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginPermissionGrants {
    pub network_hosts: Vec<String>,
    pub filesystem_read: Vec<PathBuf>,
    pub filesystem_write: Vec<PathBuf>,
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
    /// 插件 manifest 文件。每个文件会在启动时加载。
    pub plugin_manifests: Vec<PathBuf>,
}

/// 配置文件中的资源类型定义。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceKindConfig {
    /// 资源类型值，例如 `core:image`。
    pub kind: String,
    /// 可选父类型；父类型必须由内置、配置或插件定义。
    pub parent: Option<String>,
    /// 展示名称；为空时使用 `kind`。
    pub label: Option<String>,
    /// 是否支持对象内容。
    pub supports_content: bool,
    /// 文件自动识别规则。上传时前端可用这些规则自动选择 kind。
    pub detect: ResourceContentMatcher,
    /// kind 支持的动作，例如 `read`、`thumbnail`。
    pub actions: Vec<ResourceActionDefinition>,
}

/// 插件对已有资源类型的动作扩展。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceKindExtensionConfig {
    /// 被扩展的资源类型。
    pub kind: String,
    /// 扩展级匹配条件，会作为默认条件应用到未声明内容匹配条件的 action 上。
    pub content: ResourceContentMatcher,
    /// 追加到目标 kind 的动作。
    pub actions: Vec<ResourceActionDefinition>,
}

impl Default for ResourceKindConfig {
    fn default() -> Self {
        Self {
            kind: String::new(),
            parent: None,
            label: None,
            supports_content: true,
            detect: ResourceContentMatcher::default(),
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

fn normalize_permission_grant(path: &Path) -> Result<PathBuf, CoreError> {
    let path = normalize_path(path)?;
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(CoreError::configuration(format!(
            "plugin filesystem grant `{}` must be canonical",
            path.display()
        )));
    }
    Ok(path)
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
        assert!(config.kind.plugin_manifests.is_empty());
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
    fn kind_config_accepts_static_definitions_and_plugin_manifests() {
        let config = AssetInfraConfig::from_config_str(
            r#"
            [kind]
            plugin_manifests = ["plugins/example.json"]

            [[kind.definitions]]
            kind = "doc:note"
            label = "Note"
            supports_content = false
            "#,
        )
        .unwrap();

        assert_eq!(
            config.kind.plugin_manifests,
            [PathBuf::from("plugins/example.json")]
        );
        assert_eq!(config.kind.definitions.len(), 1);
        assert_eq!(config.kind.definitions[0].kind, "doc:note");
        assert_eq!(config.kind.definitions[0].label.as_deref(), Some("Note"));
        assert!(!config.kind.definitions[0].supports_content);
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
        assert!(config.kind.plugin_manifests.is_empty());
    }

    #[test]
    fn normalized_config_turns_plugin_manifests_into_absolute_paths() {
        let config = AssetInfraConfig {
            kind: KindRegistryConfig {
                plugin_manifests: vec![PathBuf::from("plugins/example.json")],
                ..KindRegistryConfig::default()
            },
            ..AssetInfraConfig::default()
        }
        .normalized()
        .unwrap();

        assert!(config.kind.plugin_manifests[0].is_absolute());
    }

    #[test]
    fn plugin_host_policy_parses_budgets_and_normalizes_grants() {
        let config = AssetInfraConfig::from_config_str(
            r#"
            [plugin]
            max_content_bytes = 1024
            max_inline_content_bytes = 512
            max_content_read_bytes = 256
            max_input_bytes = 2048
            max_output_bytes = 4096
            max_concurrent_calls = 2
            memory_max_pages = 128
            timeout_seconds = 5

            [plugin.grants]
            network_hosts = ["api.example.com"]
            filesystem_read = ["plugin-data"]
            filesystem_write = []
            "#,
        )
        .unwrap()
        .normalized()
        .unwrap();

        assert_eq!(config.plugin.max_concurrent_calls, 2);
        assert_eq!(
            config
                .plugin
                .execution_policy()
                .unwrap()
                .max_content_bytes(),
            1024
        );
        assert_eq!(config.plugin.grants.network_hosts, ["api.example.com"]);
        assert!(config.plugin.grants.filesystem_read[0].is_absolute());
    }

    #[test]
    fn plugin_host_policy_rejects_unbounded_or_zero_values() {
        let wildcard = AssetInfraConfig::from_config_str(
            r#"
            [plugin.grants]
            network_hosts = ["*"]
            "#,
        )
        .unwrap()
        .normalized();
        assert!(wildcard.is_err());

        let zero = AssetInfraConfig::from_config_str(
            r#"
            [plugin]
            max_concurrent_calls = 0
            "#,
        )
        .unwrap()
        .normalized();
        assert!(zero.is_err());
    }

    #[test]
    fn configured_content_limit_is_the_runtime_policy_limit() {
        let config = AssetInfraConfig::from_config_str(
            r#"
            [plugin]
            max_content_bytes = 134217728
            "#,
        )
        .unwrap()
        .normalized()
        .unwrap();

        let policy = config.plugin.execution_policy().unwrap();
        assert_eq!(policy.max_content_bytes(), 128 * 1024 * 1024);
        assert_eq!(
            policy.max_inline_content_bytes(),
            DEFAULT_PLUGIN_MAX_INLINE_CONTENT_BYTES
        );
    }
}
