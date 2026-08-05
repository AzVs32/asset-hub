use ::config::{Config, File, FileFormat};
use asset_core::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod blob_config;
mod database_config;
mod plugin_host_config;
mod resource_edit_config;

pub use blob_config::{BlobBackend, BlobConfig, LocalBlobConfig, LocalBlobSyncConfig};
pub use database_config::{DatabaseBackend, DatabaseConfig, SqliteDatabaseConfig};
pub use plugin_host_config::{PluginHostConfig, PluginPermissionGrants};
pub use resource_edit_config::ResourceEditConfig;

/// 默认配置文件名。
const DEFAULT_CONFIG_FILE: &str = "config.toml";
/// 默认本地 Blob 存储根目录。
const DEFAULT_LOCAL_BLOB_ROOT: &str = "data";
/// 默认本地存储同步事件去抖时间，单位为毫秒。
const DEFAULT_LOCAL_SYNC_DEBOUNCE_MILLISECONDS: u64 = 1_000;
/// 默认本地存储全量协调间隔，单位为秒。
const DEFAULT_LOCAL_SYNC_INTERVAL_SECONDS: u64 = 30 * 60;
/// SQLite 数据库在本地 Blob 存储根目录中的固定相对路径。
const SQLITE_DATABASE_RELATIVE_PATH: &str = ".asset-hub/asset-hub.sqlite";
/// 插件安装目录在本地 Blob 根目录中的固定相对路径。
const PLUGIN_PACKAGES_RELATIVE_PATH: &str = ".asset-hub/plugins";
/// 默认 SQLite 连接池最大连接数。
const DEFAULT_SQLITE_MAX_CONNECTIONS: u32 = 5;
/// 默认单次插件动作可处理的资源内容最大字节数。
const DEFAULT_PLUGIN_MAX_CONTENT_BYTES: u64 = 64 * 1024 * 1024;
/// 默认允许直接内联到插件 JSON 请求中的资源内容最大字节数。
const DEFAULT_PLUGIN_MAX_INLINE_CONTENT_BYTES: u64 = 4 * 1024 * 1024;
/// 默认插件内容 ABI 单次读取允许返回的最大字节数。
const DEFAULT_PLUGIN_MAX_CONTENT_READ_BYTES: u64 = 4 * 1024 * 1024;
/// 默认插件调用序列化输入的最大字节数。
const DEFAULT_PLUGIN_MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
/// 默认插件调用序列化输出的最大字节数。
const DEFAULT_PLUGIN_MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// 默认允许同时执行的插件调用数量。
const DEFAULT_PLUGIN_MAX_CONCURRENT_CALLS: usize = 8;
/// 默认单个插件实例可使用的 WebAssembly 最大内存页数。
const DEFAULT_PLUGIN_MEMORY_MAX_PAGES: u32 = 4096;
/// 默认单次插件调用超时时间，单位为秒。
const DEFAULT_PLUGIN_TIMEOUT_SECONDS: u64 = 20;
/// 默认交互式文本编辑最大字节数。
const DEFAULT_RESOURCE_EDIT_MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;

/// 基础设施配置。
///
/// 该配置由 `config` crate 加载，并允许配置文件为空。空文件或缺失字段都会使用默认值：
/// - 数据库后端：`sqlite`
/// - Blob 存储后端：`local`
/// - 本地 Blob 存储根目录：`data`
/// - SQLite 数据库文件：固定为 `<blob.local.root>/.asset-hub/asset-hub.sqlite`
/// - SQLite 最大连接数：`5`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AssetInfraConfig {
    /// 数据存储配置。
    pub database: DatabaseConfig,
    /// 对象存储配置。
    pub blob: BlobConfig,
    /// 插件执行预算和宿主批准的外部权限。
    pub plugin: PluginHostConfig,
    /// Host 交互式资源编辑策略。
    pub resource_edit: ResourceEditConfig,
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
    /// 工作目录转换成绝对路径。SQLite 和插件安装目录始终由归一化后的 Blob 根目录派生。
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
        self.plugin.normalize_and_validate()?;
        self.resource_edit.validate()?;
        Ok(self)
    }

    /// 返回由本地 Blob 根目录派生的固定 SQLite 数据库文件路径。
    ///
    /// 数据库必须与 Blob 数据域一起迁移，因此不允许单独配置路径。
    pub fn sqlite_path(&self) -> PathBuf {
        match self.blob.backend {
            BlobBackend::Local => self.blob.local.root.join(SQLITE_DATABASE_RELATIVE_PATH),
        }
    }

    /// 返回约定的插件安装目录 `<blob.local.root>/.asset-hub/plugins`。
    pub fn plugin_packages_path(&self) -> PathBuf {
        match self.blob.backend {
            BlobBackend::Local => self.blob.local.root.join(PLUGIN_PACKAGES_RELATIVE_PATH),
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
