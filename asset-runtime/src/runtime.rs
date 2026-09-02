use crate::upload_finalization::UploadFinalizationScheduler;
use crate::{PluginWebAssets, UploadFinalizationDispatcher};
use asset_core::CoreError;
use asset_core::domain::{ResourceActionPolicy, ResourceContentEditPolicy};
use asset_core::service::{
    ActionOrchestrator, AssetWorkflowService, AuthorizationService, ContentService,
    DirectoryIndexService, DirectoryProvisioningService, DirectoryService, DirectoryServices,
    ResourceService, ResourceServices, StorageMaintenanceService, UploadService, UserService,
};
use asset_infra::AssetInfrastructure;
use asset_infra::action::{DefaultDirectoryActionExecutor, DefaultResourceActionExecutor};
use asset_infra::config::{AssetInfraConfig, BlobBackend};
use asset_infra::kind::build_capability_catalogs;
use asset_infra::password::Argon2PasswordHasher;
use asset_infra::plugin::{ExtismActionExecutor, ExtismHost};
use asset_infra::plugin_package::PluginCatalog;
use asset_infra::storage::LocalStorageSync;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 应用运行时。
///
/// `AssetRuntime` 负责根据调用方已经加载的配置组装基础设施与核心 service，并持有由
/// 应用入口显式启动的后台任务。配置来源、命令行参数和传输层生命周期由各应用自行决定。
pub struct AssetRuntime {
    /// 已验证的浏览器静态资源快照
    plugin_web_assets: PluginWebAssets,
    resource_service: ResourceService,
    content_service: ContentService,
    upload_service: UploadService,
    action_orchestrator: ActionOrchestrator,
    storage_maintenance_service: StorageMaintenanceService,
    directory_service: DirectoryService,
    directory_provisioning_service: DirectoryProvisioningService,
    directory_index_service: DirectoryIndexService,
    asset_workflow_service: AssetWorkflowService,
    user_service: UserService,
    /// 授权应用能力
    authorization_service: AuthorizationService,
    /// 持有 supervisor 和子任务生命周期
    upload_finalizations: Arc<UploadFinalizationScheduler>,
    /// 启动同步所需的最小 effective settings
    storage_sync_settings: Option<StorageSyncSettings>,
    /// 保持自动存储同步监听器与后台任务存活。
    storage_sync: Option<LocalStorageSync>,
}

struct StorageSyncSettings {
    root: PathBuf,
    debounce: Duration,
    reconcile_interval: Duration,
}

impl AssetRuntime {
    /// 使用调用方提供的配置组装应用运行时。
    ///
    /// 创建运行时时不会自动启动后台任务；长生命周期应用应按需显式调用
    /// [`AssetRuntime::start_storage_sync`]。
    pub async fn new(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let infrastructure = AssetInfrastructure::new(config).await?;
        let config = infrastructure.config();
        let storage_sync_settings = match config.blob.backend {
            BlobBackend::Local if config.blob.local.sync.enabled => Some(StorageSyncSettings {
                root: config.blob.local.root.clone(),
                debounce: Duration::from_millis(config.blob.local.sync.debounce_milliseconds),
                reconcile_interval: Duration::from_secs(
                    config.blob.local.sync.reconcile_interval_seconds,
                ),
            }),
            BlobBackend::Local => None,
        };
        let catalog_started = Instant::now();
        let plugin_catalog = PluginCatalog::load(&config.plugin_packages_path())?;
        tracing::info!(
            elapsed_ms = catalog_started.elapsed().as_millis(),
            plugins = plugin_catalog.plugin_count(),
            "plugin artifacts verified"
        );

        let capability_catalogs = build_capability_catalogs(&plugin_catalog)?;
        let resource_kind_registry = Arc::new(capability_catalogs.resource_kinds);
        let directory_kind_registry = Arc::new(capability_catalogs.directory_kinds);
        let resource_action_registry = Arc::new(capability_catalogs.resource_actions);
        let directory_action_registry = Arc::new(capability_catalogs.directory_actions);
        let plugin_execution_policy = Arc::new(config.plugin.execution_policy()?);
        let resource_action_policy = Arc::new(
            ResourceActionPolicy::new(
                plugin_execution_policy.max_content_bytes(),
                plugin_execution_policy.max_inline_content_bytes(),
            )
            .map_err(|error| CoreError::configuration(error.to_string()))?,
        );
        let resource_content_edit_policy = Arc::new(
            ResourceContentEditPolicy::new(config.resource_edit.max_text_bytes)
                .map_err(|error| CoreError::configuration(error.to_string()))?,
        );

        let compile_started = Instant::now();
        let extism_action_executor = ExtismActionExecutor::from_catalog(
            &plugin_catalog,
            resource_kind_registry.as_ref(),
            directory_kind_registry.as_ref(),
            ExtismHost::new(
                infrastructure.directory_query(),
                infrastructure.resource_read_model(),
                infrastructure.blob_storage(),
                plugin_execution_policy.clone(),
                config.plugin.grants.clone(),
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

        let directory_services = DirectoryServices::new(
            infrastructure.directory_store(),
            infrastructure.directory_index(),
            infrastructure.directory_storage(),
            infrastructure.directory_relocation_store(),
            directory_kind_registry,
        );
        let directory_service = directory_services
            .directory_service()
            .with_actions(directory_action_registry, directory_action_executor);
        let directory_provisioning_service = directory_services.provisioning_service();
        let directory_index_service = directory_services.index_service();
        let recovered_relocations = directory_service.recover_pending_relocations().await?;
        if recovered_relocations > 0 {
            tracing::info!(
                count = recovered_relocations,
                "recovered pending directory relocations"
            );
        }
        let resource_services = ResourceServices::new(
            infrastructure.resource_store(),
            infrastructure.resource_read_model(),
            infrastructure.resource_maintenance_read_model(),
            infrastructure.resource_relocation_store(),
            infrastructure.blob_storage(),
            infrastructure.storage_scanner(),
            directory_service.clone(),
            directory_provisioning_service.clone(),
            resource_kind_registry,
            infrastructure.upload_session_repository(),
            infrastructure.content_replacement_repository(),
            resource_action_registry,
            resource_action_executor,
            resource_action_policy,
            resource_content_edit_policy,
        );
        let resource_service = resource_services.resource_service();
        let content_service = resource_services.content_service();
        let upload_service = resource_services.upload_service();
        let action_orchestrator = resource_services.action_orchestrator();
        let storage_maintenance_service = resource_services.storage_maintenance_service();
        let recovered_resource_relocations = resource_service.recover_pending_relocations().await?;
        if recovered_resource_relocations > 0 {
            tracing::info!(
                count = recovered_resource_relocations,
                "recovered pending resource relocations"
            );
        }
        let user_service = UserService::new(
            infrastructure.user_repository(),
            infrastructure.user_query(),
            Arc::new(Argon2PasswordHasher),
            directory_provisioning_service.clone(),
        );
        let authorization_service =
            AuthorizationService::new(infrastructure.user_repository(), directory_service.clone());
        let asset_workflow_service = AssetWorkflowService::new(
            resource_service.clone(),
            upload_service.clone(),
            action_orchestrator.clone(),
            directory_service.clone(),
        );
        let plugin_web_assets = plugin_web_assets_from_catalog(&plugin_catalog)?;

        let replacements_resumed = content_service.resume_pending_replacements().await?;
        if replacements_resumed > 0 {
            tracing::info!(
                count = replacements_resumed,
                "recovered pending content replacements"
            );
        }
        let pending_finalizations = upload_service.pending_finalizations().await?;
        let resumed = pending_finalizations.len();
        let upload_finalizations =
            Arc::new(UploadFinalizationScheduler::new(upload_service.clone()));
        for id in pending_finalizations {
            upload_finalizations.dispatch(id)?;
        }
        if resumed > 0 {
            tracing::info!(count = resumed, "scheduled pending upload finalizations");
        }
        Ok(Self {
            plugin_web_assets,
            resource_service,
            content_service,
            upload_service,
            action_orchestrator,
            storage_maintenance_service,
            directory_service,
            directory_provisioning_service,
            directory_index_service,
            asset_workflow_service,
            user_service,
            authorization_service,
            upload_finalizations,
            storage_sync_settings,
            storage_sync: None,
        })
    }

    /// 启动配置所指定的自动存储同步任务，并由运行时持有其生命周期。
    ///
    /// 文件监听器会在返回前建立；首次全盘协调和校验和计算在后台执行，不阻塞应用监听端口。
    ///
    /// 重复调用不会创建第二个同步任务。配置禁用同步时该方法成功返回但不启动任务。
    pub async fn start_storage_sync(&mut self) -> Result<(), CoreError> {
        if self.storage_sync.is_none()
            && let Some(settings) = &self.storage_sync_settings
        {
            self.storage_sync = Some(
                LocalStorageSync::start(
                    settings.root.clone(),
                    settings.debounce,
                    settings.reconcile_interval,
                    self.storage_maintenance_service.clone(),
                )
                .await?,
            );
        }
        Ok(())
    }

    /// 返回 Resource 聚合应用服务。
    pub fn resource_service(&self) -> ResourceService {
        self.resource_service.clone()
    }

    pub fn content_service(&self) -> ContentService {
        self.content_service.clone()
    }

    pub fn upload_service(&self) -> UploadService {
        self.upload_service.clone()
    }

    pub fn action_orchestrator(&self) -> ActionOrchestrator {
        self.action_orchestrator.clone()
    }

    pub fn storage_maintenance_service(&self) -> StorageMaintenanceService {
        self.storage_maintenance_service.clone()
    }

    pub fn directory_service(&self) -> DirectoryService {
        self.directory_service.clone()
    }

    pub fn directory_provisioning_service(&self) -> DirectoryProvisioningService {
        self.directory_provisioning_service.clone()
    }

    pub fn directory_index_service(&self) -> DirectoryIndexService {
        self.directory_index_service.clone()
    }

    pub fn asset_workflow_service(&self) -> AssetWorkflowService {
        self.asset_workflow_service.clone()
    }

    pub fn user_service(&self) -> UserService {
        self.user_service.clone()
    }

    pub fn authorization_service(&self) -> AuthorizationService {
        self.authorization_service.clone()
    }

    /// 返回供 Application Surface 提交上传最终化工作的窄 Host capability。
    pub fn upload_finalization_dispatcher(&self) -> Arc<dyn UploadFinalizationDispatcher> {
        self.upload_finalizations.clone()
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
