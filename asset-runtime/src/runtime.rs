use crate::{PluginWebAssets, UploadFinalizationScheduler};
use asset_core::CoreError;
use asset_core::domain::{ResourceActionPolicy, ResourceContentEditPolicy};
use asset_core::port::{DirectoryKindRegistry, ResourceKindRegistry};
use asset_core::service::{
    AuthorizationService, DirectoryService, ResourceService, ResourceServicePorts, UserService,
};
use asset_infra::AssetInfrastructure;
use asset_infra::action::{DefaultDirectoryActionExecutor, DefaultResourceActionExecutor};
use asset_infra::config::AssetInfraConfig;
use asset_infra::kind::{
    DefaultDirectoryActionRegistry, DefaultDirectoryKindRegistry, DefaultResourceActionRegistry,
    DefaultResourceKindRegistry, directory_action_registry_from_catalog, registries_from_catalog,
};
use asset_infra::password::Argon2PasswordHasher;
use asset_infra::plugin::{ExtismActionExecutor, ExtismHost};
use asset_infra::plugin_package::PluginCatalog;
use asset_infra::storage::LocalStorageSync;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// 应用运行时。
///
/// `AssetRuntime` 负责根据调用方已经加载的配置组装基础设施与核心 service，并持有由
/// 应用入口显式启动的后台任务。配置来源、命令行参数和传输层生命周期由各应用自行决定。
pub struct AssetRuntime {
    /// 已初始化的基础设施组合。
    infrastructure: AssetInfrastructure,
    _plugin_catalog: PluginCatalog,
    resource_kind_registry: Arc<DefaultResourceKindRegistry>,
    directory_kind_registry: Arc<DefaultDirectoryKindRegistry>,
    _resource_action_registry: Arc<DefaultResourceActionRegistry>,
    _directory_action_registry: Arc<DefaultDirectoryActionRegistry>,
    _resource_action_executor: Arc<DefaultResourceActionExecutor>,
    _directory_action_executor: Arc<DefaultDirectoryActionExecutor>,
    plugin_web_assets: PluginWebAssets,
    resource_service: ResourceService,
    user_service: UserService,
    authorization_service: AuthorizationService,
    upload_finalizations: UploadFinalizationScheduler,
    /// 保持自动存储同步监听器与后台任务存活。
    storage_sync: Option<LocalStorageSync>,
}

impl AssetRuntime {
    /// 使用调用方提供的配置组装应用运行时。
    ///
    /// 创建运行时时不会自动启动后台任务；长生命周期应用应按需显式调用
    /// [`AssetRuntime::start_storage_sync`]。
    pub async fn new(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let infrastructure = AssetInfrastructure::new(config).await?;
        let catalog_started = Instant::now();
        let plugin_catalog = PluginCatalog::load(&infrastructure.config().plugin_packages_path())?;
        tracing::info!(
            elapsed_ms = catalog_started.elapsed().as_millis(),
            plugins = plugin_catalog.plugin_count(),
            "plugin artifacts verified"
        );

        let (resource_kind_registry, directory_kind_registry, resource_action_registry) =
            registries_from_catalog(&plugin_catalog)?;
        let resource_kind_registry = Arc::new(resource_kind_registry);
        let directory_kind_registry = Arc::new(directory_kind_registry);
        let resource_action_registry = Arc::new(resource_action_registry);
        let directory_action_registry = Arc::new(directory_action_registry_from_catalog(
            &plugin_catalog,
            directory_kind_registry.as_ref(),
        )?);
        let plugin_execution_policy = Arc::new(infrastructure.config().plugin.execution_policy()?);
        let resource_action_policy = Arc::new(
            ResourceActionPolicy::new(
                plugin_execution_policy.max_content_bytes(),
                plugin_execution_policy.max_inline_content_bytes(),
            )
            .map_err(|error| CoreError::configuration(error.to_string()))?,
        );
        let resource_content_edit_policy = Arc::new(
            ResourceContentEditPolicy::new(infrastructure.config().resource_edit.max_text_bytes)
                .map_err(|error| CoreError::configuration(error.to_string()))?,
        );

        let compile_started = Instant::now();
        let extism_action_executor = ExtismActionExecutor::from_catalog(
            &plugin_catalog,
            resource_kind_registry.as_ref(),
            directory_kind_registry.as_ref(),
            ExtismHost::new(
                infrastructure.directory_query(),
                infrastructure.resource_query(),
                infrastructure.blob_storage(),
                plugin_execution_policy.clone(),
                infrastructure.config().plugin.grants.clone(),
            ),
        )?;
        let directory_action_executor = Arc::new(DefaultDirectoryActionExecutor::new(
            &plugin_catalog,
            directory_kind_registry.as_ref(),
            extism_action_executor.clone(),
        ));
        let resource_action_executor = Arc::new(DefaultResourceActionExecutor::new(
            &plugin_catalog,
            resource_kind_registry.as_ref(),
            extism_action_executor,
        ));
        tracing::info!(
            elapsed_ms = compile_started.elapsed().as_millis(),
            "plugins compiled"
        );

        let directory_service = DirectoryService::new(
            infrastructure.directory_store(),
            infrastructure.directory_index(),
            infrastructure.directory_storage(),
            directory_kind_registry.clone(),
        )
        .with_actions(
            directory_action_registry.clone(),
            directory_action_executor.clone(),
        );
        let resource_service = ResourceService::new(
            ResourceServicePorts::new(
                infrastructure.resource_repository(),
                infrastructure.resource_query(),
                infrastructure.blob_storage(),
                infrastructure.storage_scanner(),
                resource_kind_registry.clone(),
                infrastructure.upload_session_repository(),
                infrastructure.content_replacement_repository(),
            )
            .with_actions(
                resource_action_registry.clone(),
                resource_action_executor.clone(),
            ),
            directory_service.clone(),
            resource_action_policy,
            resource_content_edit_policy,
        );
        let user_service = UserService::new(
            infrastructure.user_repository(),
            infrastructure.user_query(),
            Arc::new(Argon2PasswordHasher),
            directory_service.clone(),
        );
        let authorization_service =
            AuthorizationService::new(infrastructure.user_repository(), directory_service);
        let plugin_web_assets = plugin_web_assets_from_catalog(&plugin_catalog)?;

        let replacements_resumed = resource_service.resume_content_replacements().await?;
        if replacements_resumed > 0 {
            tracing::info!(
                count = replacements_resumed,
                "recovered pending content replacements"
            );
        }
        let pending_finalizations = resource_service.pending_upload_finalizations().await?;
        let resumed = pending_finalizations.len();
        let upload_finalizations = UploadFinalizationScheduler::new(resource_service.clone());
        for id in pending_finalizations {
            upload_finalizations.schedule(id)?;
        }
        if resumed > 0 {
            tracing::info!(count = resumed, "scheduled pending upload finalizations");
        }
        Ok(Self {
            infrastructure,
            _plugin_catalog: plugin_catalog,
            resource_kind_registry,
            directory_kind_registry,
            _resource_action_registry: resource_action_registry,
            _directory_action_registry: directory_action_registry,
            _resource_action_executor: resource_action_executor,
            _directory_action_executor: directory_action_executor,
            plugin_web_assets,
            resource_service,
            user_service,
            authorization_service,
            upload_finalizations,
            storage_sync: None,
        })
    }

    /// 启动配置所指定的自动存储同步任务，并由运行时持有其生命周期。
    ///
    /// 文件监听器会在返回前建立；首次全盘协调和校验和计算在后台执行，不阻塞应用监听端口。
    ///
    /// 重复调用不会创建第二个同步任务。配置禁用同步时该方法成功返回但不启动任务。
    pub async fn start_storage_sync(&mut self) -> Result<(), CoreError> {
        if self.storage_sync.is_none() {
            self.storage_sync = self
                .infrastructure
                .start_storage_sync(self.resource_service.clone())
                .await?;
        }
        Ok(())
    }

    /// 返回实际生效的基础设施配置。
    pub fn config(&self) -> &AssetInfraConfig {
        self.infrastructure.config()
    }

    /// 创建资源应用服务。
    pub fn resource_service(&self) -> ResourceService {
        self.resource_service.clone()
    }

    pub fn user_service(&self) -> UserService {
        self.user_service.clone()
    }

    pub fn authorization_service(&self) -> AuthorizationService {
        self.authorization_service.clone()
    }

    pub fn upload_finalization_scheduler(&self) -> UploadFinalizationScheduler {
        self.upload_finalizations.clone()
    }

    /// 返回由基础设施统一创建的数据库连接池。
    pub fn database_pool(&self) -> SqlitePool {
        self.infrastructure.database_pool()
    }

    /// 返回资源类型注册表。
    pub fn resource_kind_registry(&self) -> Arc<dyn ResourceKindRegistry> {
        self.resource_kind_registry.clone()
    }

    /// 返回目录类型注册表。
    pub fn directory_kind_registry(&self) -> Arc<dyn DirectoryKindRegistry> {
        self.directory_kind_registry.clone()
    }

    /// 返回启动时校验并冻结的插件浏览器静态资源。
    pub fn plugin_web_assets(&self) -> PluginWebAssets {
        self.plugin_web_assets.clone()
    }
}

fn plugin_web_assets_from_catalog(catalog: &PluginCatalog) -> Result<PluginWebAssets, CoreError> {
    let mut assets = HashMap::new();
    for plugin in catalog.plugins() {
        if plugin.web_assets().is_empty() {
            continue;
        }
        let plugin_id = plugin.manifest().plugin_id();
        if assets
            .insert(plugin_id.to_string(), plugin.web_assets().clone())
            .is_some()
        {
            return Err(CoreError::configuration(format!(
                "duplicate plugin Web root `{plugin_id}`"
            )));
        }
    }
    Ok(assets)
}

#[cfg(test)]
mod tests;
