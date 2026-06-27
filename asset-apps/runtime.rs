use asset_core::CoreError;
use asset_core::service::ResourceService;
use asset_infra::AssetInfrastructure;
use asset_infra::config::AssetInfraConfig;
use std::path::Path;

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

    /// 使用显式配置创建应用运行时。
    pub async fn from_config(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let infrastructure = AssetInfrastructure::new(config).await?;

        Ok(Self { infrastructure })
    }

    /// 使用可选配置文件创建应用运行时。
    ///
    /// `path` 为 `None` 时使用默认配置；配置文件存在但内容为空时，也会使用默认配置。
    pub async fn from_optional_config_file(
        path: Option<impl AsRef<Path>>,
    ) -> Result<Self, CoreError> {
        match path {
            Some(path) => Self::from_config(AssetInfraConfig::from_toml_file(path)?).await,
            None => Self::with_defaults().await,
        }
    }

    /// 返回实际生效的基础设施配置。
    pub fn config(&self) -> &AssetInfraConfig {
        self.infrastructure.config()
    }

    /// 创建资源应用服务。
    pub fn resource_service(&self) -> ResourceService {
        self.infrastructure.resource_service()
    }
}
