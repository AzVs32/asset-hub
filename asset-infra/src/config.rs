use ::config::{Config, File, FileFormat};
use asset_core::CoreError;
use asset_plugin_api::{PluginExecutionPolicy, ResourceActionDefinition, ResourceContentMatcher};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 默认配置文件名。
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
/// 默认本地 Blob 存储根目录。
pub const DEFAULT_LOCAL_BLOB_ROOT: &str = "data";
pub const DEFAULT_LOCAL_SYNC_DEBOUNCE_MILLISECONDS: u64 = 1_000;
pub const DEFAULT_LOCAL_SYNC_INTERVAL_SECONDS: u64 = 30 * 60;
/// SQLite 数据库在本地 Blob 存储根目录中的固定相对路径。
const SQLITE_DATABASE_RELATIVE_PATH: &str = ".asset-hub/asset-hub.sqlite";
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
/// - 数据库后端：`sqlite`
/// - Blob 存储后端：`local`
/// - 本地 Blob 存储根目录：`data`
/// - SQLite 数据库文件：`<blob.local.root>/.asset-hub/asset-hub.sqlite`
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
    /// 当前主要处理路径：本地 Blob 根目录可以在配置中写相对路径，归一化后会基于当前
    /// 工作目录转换成绝对路径。SQLite 文件始终位于该根目录下的固定内部路径。
    pub fn normalized(mut self) -> Result<Self, CoreError> {
        match self.database.backend {
            DatabaseBackend::Sqlite => {
                if self.database.sqlite.max_connections == 0 {
                    return Err(CoreError::configuration(
                        "database.sqlite.max_connections must be greater than 0",
                    ));
                }
            }
        }
        match self.blob.backend {
            BlobBackend::Local => {
                self.blob.local.root = normalize_path(&self.blob.local.root)?;
                self.blob.local.sync.validate()?;
            }
        }
        self.kind.plugin_manifests = self
            .kind
            .plugin_manifests
            .iter()
            .map(|path| normalize_path(path))
            .collect::<Result<Vec<_>, _>>()?;
        self.plugin.normalize_and_validate()?;
        Ok(self)
    }

    /// 返回由本地 Blob 存储根目录派生的固定 SQLite 数据库文件路径。
    pub fn sqlite_path(&self) -> PathBuf {
        match self.blob.backend {
            BlobBackend::Local => self.blob.local.root.join(SQLITE_DATABASE_RELATIVE_PATH),
        }
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

/// 对象存储配置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlobConfig {
    /// 当前启用的 Blob 存储后端。
    pub backend: BlobBackend,
    /// 本地文件系统后端专属配置。
    pub local: LocalBlobConfig,
}

/// 可用的 Blob 存储后端。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlobBackend {
    #[default]
    Local,
}

/// 本地文件系统 Blob 后端专属配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalBlobConfig {
    /// 本地存储根目录。相对路径会在初始化时按当前工作目录转换为绝对路径。
    pub root: PathBuf,
    /// 本地文件系统与 Resource 数据库的自动同步策略。
    pub sync: LocalBlobSyncConfig,
}

impl Default for LocalBlobConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from(DEFAULT_LOCAL_BLOB_ROOT),
            sync: LocalBlobSyncConfig::default(),
        }
    }
}

/// 本地 Blob 存储自动同步配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalBlobSyncConfig {
    /// 是否启动文件系统监听和后台协调。默认启用。
    pub enabled: bool,
    /// 文件系统事件合并窗口，避免一次保存触发多次 checksum 计算。
    pub debounce_milliseconds: u64,
    /// 保底全量协调周期，用于纠正程序停机或平台事件丢失造成的偏差。
    pub reconcile_interval_seconds: u64,
}

impl Default for LocalBlobSyncConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_milliseconds: DEFAULT_LOCAL_SYNC_DEBOUNCE_MILLISECONDS,
            reconcile_interval_seconds: DEFAULT_LOCAL_SYNC_INTERVAL_SECONDS,
        }
    }
}

impl LocalBlobSyncConfig {
    fn validate(&self) -> Result<(), CoreError> {
        if self.enabled && self.debounce_milliseconds == 0 {
            return Err(CoreError::configuration(
                "blob.local.sync.debounce_milliseconds must be greater than 0",
            ));
        }
        if self.enabled && self.reconcile_interval_seconds == 0 {
            return Err(CoreError::configuration(
                "blob.local.sync.reconcile_interval_seconds must be greater than 0",
            ));
        }
        Ok(())
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
    /// 以该 kind 为作用域声明的动作；启动时会统一注册到 `ResourceActionRegistry`。
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
        .set_default("database.backend", "sqlite")
        .expect("default database backend should be a valid config value")
        .set_default(
            "database.sqlite.max_connections",
            i64::from(DEFAULT_SQLITE_MAX_CONNECTIONS),
        )
        .expect("default sqlite max connections should be a valid config value")
        .set_default("blob.backend", "local")
        .expect("default blob backend should be a valid config value")
        .set_default("blob.local.root", DEFAULT_LOCAL_BLOB_ROOT)
        .expect("default local blob root should be a valid config value")
        .set_default("blob.local.sync.enabled", true)
        .expect("default local sync enabled should be a valid config value")
        .set_default(
            "blob.local.sync.debounce_milliseconds",
            DEFAULT_LOCAL_SYNC_DEBOUNCE_MILLISECONDS as i64,
        )
        .expect("default local sync debounce should be a valid config value")
        .set_default(
            "blob.local.sync.reconcile_interval_seconds",
            DEFAULT_LOCAL_SYNC_INTERVAL_SECONDS as i64,
        )
        .expect("default local sync interval should be a valid config value")
}

fn config_error(error: ::config::ConfigError) -> CoreError {
    CoreError::configuration(error.to_string())
}

#[cfg(test)]
mod tests;
