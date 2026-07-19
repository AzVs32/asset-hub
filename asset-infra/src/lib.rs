pub mod action;
pub mod config;
pub mod kind;
pub mod migration;
pub mod official_plugins;
pub mod password;
pub mod plugin;
mod plugin_manifest;
pub mod sqlite;
pub mod storage;

use action::DefaultResourceActionExecutor;
use asset_core::service::{
    AuthorizationService, ResourceService, ResourceServicePorts, UserService,
};
use asset_core::{
    CoreError, port::BlobStorage, port::DirectoryStorage, port::ResourceActionExecutor,
    port::ResourceActionRegistry, port::ResourceKindRegistry, port::ResourceQuery,
    port::ResourceRepository, port::SecurityAuditRepository, port::StorageScanner,
};
use asset_plugin_api::PluginExecutionPolicy;
use config::AssetInfraConfig;
use kind::{DefaultResourceActionRegistry, DefaultResourceKindRegistry, registries_from_catalog};
use password::Argon2PasswordHasher;
use plugin::ExtismResourceActionExecutor;
use plugin_manifest::PluginCatalog;
use sqlite::{SqliteIdentityRepository, SqliteResourceRepository, SqliteSecurityAuditRepository};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use storage::{FileSystemScanner, OpenDalBlobStorage};

pub type PluginWebAssets = HashMap<String, HashMap<PathBuf, Arc<[u8]>>>;

/// 基于默认本地实现组装好的基础设施对象。
///
/// 当前组合是 SQLite 作为资源数据存储，OpenDAL Fs 作为对象内容存储。
pub struct AssetInfrastructure {
    /// 实际生效的基础设施配置。
    config: AssetInfraConfig,
    /// 资源仓储适配器。
    resource_repository: Arc<SqliteResourceRepository>,
    identity_repository: Arc<SqliteIdentityRepository>,
    security_audit_repository: Arc<SqliteSecurityAuditRepository>,
    /// 对象存储适配器。
    blob_storage: Arc<OpenDalBlobStorage>,
    storage_scanner: Arc<FileSystemScanner>,
    /// 资源类型注册表。
    resource_kind_registry: Arc<DefaultResourceKindRegistry>,
    /// 资源动作注册表。
    resource_action_registry: Arc<DefaultResourceActionRegistry>,
    /// 资源动作执行器。
    resource_action_executor: Arc<DefaultResourceActionExecutor>,
    plugin_execution_policy: Arc<PluginExecutionPolicy>,
    plugin_web_assets: PluginWebAssets,
}

impl AssetInfrastructure {
    /// 使用给定配置创建 SQLite + Fs 基础设施组合。
    ///
    /// 调用方可以传入 `AssetInfraConfig::default()` 使用默认本地配置。
    pub async fn new(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let config = config.normalized()?;
        let blob_storage = Arc::new(OpenDalBlobStorage::from_config(&config.blob)?);
        let storage_scanner = Arc::new(FileSystemScanner::new(config.blob.fs_root.clone()));
        let resource_repository =
            Arc::new(SqliteResourceRepository::connect(&config.database).await?);
        let identity_repository = Arc::new(SqliteIdentityRepository::new(
            resource_repository.pool().clone(),
        ));
        let security_audit_repository = Arc::new(SqliteSecurityAuditRepository::new(
            resource_repository.pool().clone(),
        ));
        let plugin_catalog = PluginCatalog::load(&config.kind)?;
        let (resource_kind_registry, resource_action_registry) =
            registries_from_catalog(&config.kind, &plugin_catalog)?;
        let resource_kind_registry = Arc::new(resource_kind_registry);
        let resource_action_registry = Arc::new(resource_action_registry);
        let plugin_execution_policy = Arc::new(config.plugin.execution_policy()?);
        let extism_action_executor = ExtismResourceActionExecutor::from_catalog(
            &plugin_catalog,
            resource_kind_registry.as_ref(),
            blob_storage.clone(),
            plugin_execution_policy.clone(),
            &config.plugin.grants,
        )?;
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

    /// 返回共享数据库连接池，供会话、用户与授权适配器复用。
    pub fn database_pool(&self) -> SqlitePool {
        self.resource_repository.pool().clone()
    }

    pub fn user_service(&self) -> UserService {
        UserService::new(
            self.identity_repository.clone(),
            Arc::new(Argon2PasswordHasher),
            self.directory_storage(),
        )
    }

    pub fn authorization_service(&self) -> AuthorizationService {
        AuthorizationService::new(self.identity_repository.clone())
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
