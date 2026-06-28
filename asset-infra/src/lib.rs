pub mod config;
pub mod kind;
pub mod migration;
pub mod sqlite;
pub mod storage;

use asset_core::service::ResourceService;
use asset_core::{
    CoreError, port::BlobStorage, port::ResourceKindRegistry, port::ResourceRepository,
};
use config::AssetInfraConfig;
use kind::DefaultResourceKindRegistry;
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

        Ok(Self {
            config,
            resource_repository,
            blob_storage,
            resource_kind_registry,
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

    /// 创建资源应用服务。
    pub fn resource_service(&self) -> ResourceService {
        ResourceService::new(
            self.resource_repository(),
            self.blob_storage(),
            self.resource_kind_registry(),
        )
    }
}
