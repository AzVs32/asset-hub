use asset_core::CoreError;
use asset_core::port::ResourceKindRegistry;
use asset_core::service::{AuthorizationService, ResourceService, UserService};
use asset_infra::AssetInfrastructure;
use asset_infra::config::{AssetInfraConfig, DEFAULT_CONFIG_FILE};
use std::path::Path;
use std::sync::Arc;

/// 应用运行时。
///
/// `AssetRuntime` 负责把配置、基础设施实现和核心 service 组装起来。
/// HTTP、CLI、TUI 等外部入口都应复用它，避免重复初始化 SQLite、Fs 等依赖。
pub struct AssetRuntime {
    /// 已初始化的基础设施组合。
    infrastructure: AssetInfrastructure,
}

impl AssetRuntime {
    /// 使用默认配置创建应用运行时。
    pub async fn with_defaults() -> Result<Self, CoreError> {
        Self::from_config(AssetInfraConfig::default()).await
    }

    /// 使用默认配置文件创建应用运行时。
    ///
    /// 当前默认配置文件名是 `config.toml`。文件不存在时使用默认配置。
    pub async fn from_default_config_file() -> Result<Self, CoreError> {
        Self::from_config(AssetInfraConfig::from_default_config_file()?).await
    }

    /// 使用显式配置创建应用运行时。
    pub async fn from_config(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let infrastructure = AssetInfrastructure::new(config).await?;

        Ok(Self { infrastructure })
    }

    /// 使用可选配置文件创建应用运行时。
    ///
    /// `path` 为 `Some` 时读取指定配置文件，文件不存在会返回错误。
    /// `path` 为 `None` 时读取默认 `config.toml`，文件不存在则使用默认配置。
    pub async fn from_optional_config_file(
        path: Option<impl AsRef<Path>>,
    ) -> Result<Self, CoreError> {
        match path {
            Some(path) => Self::from_config(AssetInfraConfig::from_config_file(path)?).await,
            None => Self::from_default_config_file().await,
        }
    }

    /// 返回默认配置文件名。
    pub fn default_config_file() -> &'static str {
        DEFAULT_CONFIG_FILE
    }

    /// 返回实际生效的基础设施配置。
    pub fn config(&self) -> &AssetInfraConfig {
        self.infrastructure.config()
    }

    /// 创建资源应用服务。
    pub fn resource_service(&self) -> ResourceService {
        self.infrastructure.resource_service()
    }

    pub fn user_service(&self) -> UserService {
        self.infrastructure.user_service()
    }

    pub fn authorization_service(&self) -> AuthorizationService {
        self.infrastructure.authorization_service()
    }

    /// 返回资源类型注册表。
    pub fn resource_kind_registry(&self) -> Arc<dyn ResourceKindRegistry> {
        self.infrastructure.resource_kind_registry()
    }

    /// 返回启动时校验并冻结的插件浏览器静态资源。
    pub fn plugin_web_assets(&self) -> asset_infra::PluginWebAssets {
        self.infrastructure.plugin_web_assets()
    }
}
