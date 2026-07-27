pub mod action;
pub mod config;
pub mod kind;
pub mod migration;
mod official_plugins;
pub mod password;
pub mod plugin;
mod plugin_manifest;
pub mod sqlite;
pub mod storage;

use action::DefaultResourceActionExecutor;
use asset_core::service::{
    AuthorizationService, DirectoryService, ResourceService, ResourceServicePorts, UserService,
};
use asset_core::{
    CoreError, port::BlobStorage, port::DirectoryKindRegistry, port::DirectoryRepository,
    port::DirectoryStorage, port::ResourceActionExecutor, port::ResourceActionRegistry,
    port::ResourceKindRegistry, port::ResourceQuery, port::ResourceRepository,
    port::SecurityAuditRepository, port::StorageScanner,
};
use asset_plugin_api::PluginExecutionPolicy;
pub use asset_plugin_api::PluginWebAssets;
use config::{AssetInfraConfig, BlobBackend, DatabaseBackend};
use kind::{
    DefaultDirectoryKindRegistry, DefaultResourceActionRegistry, DefaultResourceKindRegistry,
    registries_from_catalog,
};
use password::Argon2PasswordHasher;
use plugin::ExtismResourceActionExecutor;
use plugin_manifest::PluginCatalog;
use sqlite::{SqliteIdentityRepository, SqliteResourceRepository, SqliteSecurityAuditRepository};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use storage::{FileSystemScanner, LocalStorageSync, OpenDalBlobStorage};

/// 根据配置的后端选型组装基础设施对象。
///
/// 当前支持 SQLite 数据库和本地 Blob 存储。
pub struct AssetInfrastructure {
    /// 实际生效的基础设施配置。
    config: AssetInfraConfig,
    /// SQLite 聚合持久化适配器，对外分别实现资源与目录仓储端口。
    resource_repository: Arc<SqliteResourceRepository>,
    identity_repository: Arc<SqliteIdentityRepository>,
    security_audit_repository: Arc<SqliteSecurityAuditRepository>,
    /// 对象存储适配器。
    blob_storage: Arc<OpenDalBlobStorage>,
    storage_scanner: Arc<FileSystemScanner>,
    /// 资源类型注册表。
    resource_kind_registry: Arc<DefaultResourceKindRegistry>,
    /// 目录类型注册表。
    directory_kind_registry: Arc<DefaultDirectoryKindRegistry>,
    /// 资源动作注册表。
    resource_action_registry: Arc<DefaultResourceActionRegistry>,
    /// 资源动作执行器。
    resource_action_executor: Arc<DefaultResourceActionExecutor>,
    plugin_execution_policy: Arc<PluginExecutionPolicy>,
    plugin_web_assets: PluginWebAssets,
}

impl AssetInfrastructure {
    /// 使用给定配置创建基础设施组合。
    ///
    /// 调用方可以传入 `AssetInfraConfig::default()` 使用默认本地配置。
    pub async fn new(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let config = config.normalized()?;
        let (blob_storage, storage_scanner) = match config.blob.backend {
            BlobBackend::Local => (
                Arc::new(OpenDalBlobStorage::from_local_root(
                    &config.blob.local.root,
                )?),
                Arc::new(FileSystemScanner::new(config.blob.local.root.clone())),
            ),
        };
        let sqlite_started = Instant::now();
        let resource_repository = match config.database.backend {
            DatabaseBackend::Sqlite => {
                let sqlite_path = config.sqlite_path();
                Arc::new(
                    SqliteResourceRepository::connect(
                        &sqlite_path,
                        config.database.sqlite.max_connections,
                    )
                    .await?,
                )
            }
        };
        tracing::info!(
            elapsed_ms = sqlite_started.elapsed().as_millis(),
            "SQLite initialized"
        );
        let identity_repository = Arc::new(SqliteIdentityRepository::new(
            resource_repository.pool().clone(),
        ));
        let security_audit_repository = Arc::new(SqliteSecurityAuditRepository::new(
            resource_repository.pool().clone(),
        ));
        let plugin_catalog_started = Instant::now();
        let plugin_catalog = PluginCatalog::load(&config.kind)?;
        tracing::info!(
            elapsed_ms = plugin_catalog_started.elapsed().as_millis(),
            manifests = config.kind.plugin_manifests.len(),
            "plugin artifacts verified"
        );
        let (resource_kind_registry, directory_kind_registry, resource_action_registry) =
            registries_from_catalog(&config.kind, &plugin_catalog)?;
        let resource_kind_registry = Arc::new(resource_kind_registry);
        let directory_kind_registry = Arc::new(directory_kind_registry);
        let resource_action_registry = Arc::new(resource_action_registry);
        let plugin_execution_policy = Arc::new(config.plugin.execution_policy()?);
        let plugin_compile_started = Instant::now();
        let extism_action_executor = ExtismResourceActionExecutor::from_catalog(
            &plugin_catalog,
            resource_kind_registry.as_ref(),
            blob_storage.clone(),
            plugin_execution_policy.clone(),
            &config.plugin.grants,
        )?;
        tracing::info!(
            elapsed_ms = plugin_compile_started.elapsed().as_millis(),
            "plugins compiled"
        );
        let resource_action_executor =
            Arc::new(DefaultResourceActionExecutor::new(extism_action_executor));
        let plugin_web_assets = plugin_web_assets_from_catalog(&plugin_catalog)?;

        Ok(Self {
            config,
            resource_repository,
            identity_repository,
            security_audit_repository,
            blob_storage,
            storage_scanner,
            resource_kind_registry,
            directory_kind_registry,
            resource_action_registry,
            resource_action_executor,
            plugin_execution_policy,
            plugin_web_assets,
        })
    }

    /// 返回实际生效的基础设施配置。
    pub fn config(&self) -> &AssetInfraConfig {
        &self.config
    }

    /// 返回资源仓储端口对象。
    pub fn resource_repository(&self) -> Arc<dyn ResourceRepository> {
        self.resource_repository.clone()
    }

    pub fn resource_query(&self) -> Arc<dyn ResourceQuery> {
        self.resource_repository.clone()
    }

    pub fn directory_repository(&self) -> Arc<dyn DirectoryRepository> {
        self.resource_repository.clone()
    }

    pub fn directory_service(&self) -> DirectoryService {
        DirectoryService::new(self.directory_repository(), self.directory_storage())
    }

    /// 返回共享数据库连接池，供会话、用户与授权适配器复用。
    pub fn database_pool(&self) -> SqlitePool {
        self.resource_repository.pool().clone()
    }

    pub fn user_service(&self) -> UserService {
        UserService::new(
            self.identity_repository.clone(),
            self.identity_repository.clone(),
            Arc::new(Argon2PasswordHasher),
            self.directory_service(),
        )
    }

    pub fn authorization_service(&self) -> AuthorizationService {
        AuthorizationService::new(self.identity_repository.clone(), self.directory_service())
    }

    pub fn security_audit_repository(&self) -> Arc<dyn SecurityAuditRepository> {
        self.security_audit_repository.clone()
    }

    /// 返回对象存储端口对象。
    pub fn blob_storage(&self) -> Arc<dyn BlobStorage> {
        self.blob_storage.clone()
    }

    pub fn directory_storage(&self) -> Arc<dyn DirectoryStorage> {
        self.blob_storage.clone()
    }

    pub fn storage_scanner(&self) -> Arc<dyn StorageScanner> {
        self.storage_scanner.clone()
    }

    /// 启动当前 Blob 后端对应的自动存储同步任务。
    pub async fn start_storage_sync(
        &self,
        service: ResourceService,
    ) -> Result<Option<LocalStorageSync>, CoreError> {
        match self.config.blob.backend {
            BlobBackend::Local if self.config.blob.local.sync.enabled => LocalStorageSync::start(
                self.config.blob.local.root.clone(),
                Duration::from_millis(self.config.blob.local.sync.debounce_milliseconds),
                Duration::from_secs(self.config.blob.local.sync.reconcile_interval_seconds),
                service,
            )
            .await
            .map(Some),
            BlobBackend::Local => Ok(None),
        }
    }

    /// 返回资源类型注册表端口对象。
    pub fn resource_kind_registry(&self) -> Arc<dyn ResourceKindRegistry> {
        self.resource_kind_registry.clone()
    }

    /// 返回目录类型注册表端口对象。
    pub fn directory_kind_registry(&self) -> Arc<dyn DirectoryKindRegistry> {
        self.directory_kind_registry.clone()
    }

    /// 返回资源动作执行器端口对象。
    pub fn resource_action_executor(&self) -> Arc<dyn ResourceActionExecutor> {
        self.resource_action_executor.clone()
    }

    /// 返回全局资源动作注册表端口对象。
    pub fn resource_action_registry(&self) -> Arc<dyn ResourceActionRegistry> {
        self.resource_action_registry.clone()
    }

    /// 返回启动时校验并冻结的插件浏览器静态资源。
    pub fn plugin_web_assets(&self) -> PluginWebAssets {
        self.plugin_web_assets.clone()
    }

    /// 创建资源应用服务。
    pub fn resource_service(&self) -> ResourceService {
        ResourceService::new(
            ResourceServicePorts::new(
                self.resource_repository(),
                self.resource_query(),
                self.blob_storage(),
                self.directory_repository(),
                self.directory_storage(),
                self.storage_scanner(),
                self.resource_kind_registry(),
            )
            .with_actions(
                self.resource_action_registry(),
                self.resource_action_executor(),
            ),
            self.plugin_execution_policy.clone(),
        )
    }
}

fn plugin_web_assets_from_catalog(catalog: &PluginCatalog) -> Result<PluginWebAssets, CoreError> {
    let mut assets = HashMap::new();
    for plugin in catalog
        .plugins()
        .iter()
        .filter(|plugin| plugin.manifest_path.is_some())
    {
        let manifest = &plugin.manifest;
        if manifest.web.is_none() {
            continue;
        }
        if assets
            .insert(manifest.plugin_id().to_string(), plugin.web_assets.clone())
            .is_some()
        {
            return Err(CoreError::configuration(format!(
                "duplicate plugin Web root `{}`",
                manifest.plugin_id()
            )));
        }
    }
    Ok(assets)
}
