pub mod action;
pub mod config;
pub mod kind;
pub mod migration;
pub mod official_plugins;
pub mod plugin;
mod plugin_manifest;
pub mod sqlite;
pub mod storage;

use action::DefaultResourceActionExecutor;
use asset_core::service::ResourceService;
use asset_core::{
    CoreError, port::BlobStorage, port::ResourceActionExecutor, port::ResourceActionRegistry,
    port::ResourceKindRegistry, port::ResourceRepository,
};
use config::AssetInfraConfig;
use kind::{DefaultResourceActionRegistry, DefaultResourceKindRegistry};
use plugin::ExtismResourceActionExecutor;
use sqlite::SqliteResourceRepository;
use std::sync::Arc;
use storage::OpenDalBlobStorage;

/// 基于默认本地实现组装好的基础设施对象。
///
/// 当前组合是 SQLite 作为资源数据存储，OpenDAL Fs 作为对象内容存储。
pub struct AssetInfrastructure {
    /// 实际生效的基础设施配置。
    config: AssetInfraConfig,
    /// 资源仓储适配器。
    resource_repository: Arc<SqliteResourceRepository>,
    /// 对象存储适配器。
    blob_storage: Arc<OpenDalBlobStorage>,
    /// 资源类型注册表。
    resource_kind_registry: Arc<DefaultResourceKindRegistry>,
    /// 资源动作注册表。
    resource_action_registry: Arc<DefaultResourceActionRegistry>,
    /// 资源动作执行器。
    resource_action_executor: Arc<DefaultResourceActionExecutor>,
}

impl AssetInfrastructure {
    /// 使用给定配置创建 SQLite + Fs 基础设施组合。
    ///
    /// 调用方可以传入 `AssetInfraConfig::default()` 使用默认本地配置。
    pub async fn new(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let config = config.normalized()?;
        let blob_storage = Arc::new(OpenDalBlobStorage::from_config(&config.blob)?);
        let resource_repository =
            Arc::new(SqliteResourceRepository::connect(&config.database).await?);
        let resource_kind_registry =
            Arc::new(DefaultResourceKindRegistry::from_config(&config.kind)?);
        let resource_action_registry =
            Arc::new(DefaultResourceActionRegistry::from_config(&config.kind)?);
        let extism_action_executor = ExtismResourceActionExecutor::from_config(
            &config.kind,
            resource_kind_registry.as_ref(),
        )?;
        let resource_action_executor =
            Arc::new(DefaultResourceActionExecutor::new(extism_action_executor));

        Ok(Self {
            config,
            resource_repository,
            blob_storage,
            resource_kind_registry,
            resource_action_registry,
            resource_action_executor,
        })
    }

    /// 使用默认配置创建 SQLite + Fs 基础设施组合。
    pub async fn with_defaults() -> Result<Self, CoreError> {
        Self::new(AssetInfraConfig::default()).await
    }

    /// 返回实际生效的基础设施配置。
    pub fn config(&self) -> &AssetInfraConfig {
        &self.config
    }

    /// 返回资源仓储端口对象。
    pub fn resource_repository(&self) -> Arc<dyn ResourceRepository> {
        self.resource_repository.clone()
    }

    /// 返回对象存储端口对象。
    pub fn blob_storage(&self) -> Arc<dyn BlobStorage> {
        self.blob_storage.clone()
    }

    /// 返回资源类型注册表端口对象。
    pub fn resource_kind_registry(&self) -> Arc<dyn ResourceKindRegistry> {
        self.resource_kind_registry.clone()
    }

    /// 返回资源动作执行器端口对象。
    pub fn resource_action_executor(&self) -> Arc<dyn ResourceActionExecutor> {
        self.resource_action_executor.clone()
    }

    /// 返回全局资源动作注册表端口对象。
    pub fn resource_action_registry(&self) -> Arc<dyn ResourceActionRegistry> {
        self.resource_action_registry.clone()
    }

    /// 创建资源应用服务。
    pub fn resource_service(&self) -> ResourceService {
        ResourceService::new_with_action_registry_and_executor(
            self.resource_repository(),
            self.blob_storage(),
            self.resource_kind_registry(),
            self.resource_action_registry(),
            self.resource_action_executor(),
        )
    }
}
