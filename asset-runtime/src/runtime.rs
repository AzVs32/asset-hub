use crate::UploadFinalizationDispatcher;
use crate::upload_finalization::UploadFinalizationScheduler;
use asset_core::CoreError;
use asset_core::{
    directory::service::{DirectoryRecoveryService, DirectoryService, DirectoryServices},
    idempotency::service::IdempotencyService,
    resource::{
        domain::ResourceContentEditPolicy,
        service::{
            ContentRecoveryService, ContentService, ResourceRecoveryService, ResourceService,
            ResourceServices, StorageHealthService, StorageMaintenanceService, UploadService,
        },
    },
    workflow::service::AssetWorkflowService,
};
use asset_infra::AssetInfrastructure;
use asset_infra::config::{AssetInfraConfig, BlobBackend};
use asset_infra::storage::LocalStorageSync;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// 应用运行时。
///
/// `AssetRuntime` 负责根据调用方已经加载的配置组装基础设施与核心 service，并持有由
/// 应用入口显式启动的后台任务。配置来源、命令行参数和传输层生命周期由各应用自行决定。
pub struct AssetRuntime {
    resource_service: ResourceService,
    content_service: ContentService,
    upload_service: UploadService,
    storage_maintenance_service: StorageMaintenanceService,
    storage_health_service: StorageHealthService,
    directory_service: DirectoryService,
    directory_recovery_service: DirectoryRecoveryService,
    resource_recovery_service: ResourceRecoveryService,
    content_recovery_service: ContentRecoveryService,
    idempotency_service: IdempotencyService,
    asset_workflow_service: AssetWorkflowService,
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
        let resource_content_edit_policy = Arc::new(
            ResourceContentEditPolicy::new(config.resource_edit.max_text_bytes)
                .map_err(|error| CoreError::configuration(error.to_string()))?,
        );

        let directory_services = DirectoryServices::new(
            infrastructure.directory_store(),
            infrastructure.directory_index(),
            infrastructure.directory_storage(),
            infrastructure.directory_relocation_store(),
        );
        let directory_service = directory_services.directory_service();
        let directory_maintenance_service = directory_services.maintenance_service();
        let directory_recovery_service = directory_services.recovery_service();
        let directory_import_service = directory_services.storage_import_service();
        let directory_index_service = directory_services.index_service();
        let recovered_relocations = directory_recovery_service
            .recover_pending_relocations()
            .await?;
        if recovered_relocations > 0 {
            tracing::info!(
                count = recovered_relocations,
                "recovered pending directory relocations"
            );
        }
        let idempotency_service = IdempotencyService::with_lease_duration(
            infrastructure.idempotency_repository(),
            config.idempotency.lease_duration(),
        )?;
        let resource_services = ResourceServices::new(
            infrastructure.resource_store(),
            infrastructure.resource_read_model(),
            infrastructure.resource_maintenance_read_model(),
            infrastructure.resource_relocation_store(),
            infrastructure.resource_deletion_store(),
            infrastructure.content_reader(),
            infrastructure.content_staging_store(),
            infrastructure.content_object_store(),
            infrastructure.blob_health(),
            infrastructure.storage_scanner(),
            directory_service.clone(),
            directory_maintenance_service,
            directory_index_service.clone(),
            directory_import_service,
            infrastructure.upload_session_store(),
            infrastructure.content_replacement_store(),
            resource_content_edit_policy,
            idempotency_service.clone(),
        );
        let resource_service = resource_services.resource_service();
        let content_service = resource_services.content_service();
        let upload_service = resource_services.upload_service();
        let storage_maintenance_service = resource_services.storage_maintenance_service();
        let storage_health_service = resource_services.storage_health_service();
        let resource_recovery_service = resource_services.resource_recovery_service();
        let content_recovery_service = resource_services.content_recovery_service();
        let upload_finalization_service = resource_services.upload_finalization_service();
        let recovered_resource_relocations = resource_recovery_service
            .recover_pending_relocations()
            .await?;
        if recovered_resource_relocations > 0 {
            tracing::info!(
                count = recovered_resource_relocations,
                "recovered pending resource relocations"
            );
        }
        let recovered_resource_deletions = resource_recovery_service
            .recover_pending_deletions()
            .await?;
        if recovered_resource_deletions > 0 {
            tracing::info!(
                count = recovered_resource_deletions,
                "recovered pending resource deletions"
            );
        }
        let asset_workflow_service =
            AssetWorkflowService::new(resource_service.clone(), directory_service.clone());
        let replacements_resumed = content_recovery_service
            .resume_pending_replacements()
            .await?;
        if replacements_resumed > 0 {
            tracing::info!(
                count = replacements_resumed,
                "recovered pending content replacements"
            );
        }
        let pending_finalizations = upload_finalization_service.pending_finalizations().await?;
        let resumed = pending_finalizations.len();
        let upload_finalizations = Arc::new(UploadFinalizationScheduler::new(
            upload_finalization_service,
        ));
        for id in pending_finalizations {
            upload_finalizations.dispatch(id)?;
        }
        if resumed > 0 {
            tracing::info!(count = resumed, "scheduled pending upload finalizations");
        }
        Ok(Self {
            resource_service,
            content_service,
            upload_service,
            storage_maintenance_service,
            storage_health_service,
            directory_service,
            directory_recovery_service,
            resource_recovery_service,
            content_recovery_service,
            idempotency_service,
            asset_workflow_service,
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

    /// Return the shared, runtime-configured idempotency capability for composition-time injection.
    pub fn idempotency_service(&self) -> IdempotencyService {
        self.idempotency_service.clone()
    }

    pub fn storage_maintenance_service(&self) -> StorageMaintenanceService {
        self.storage_maintenance_service.clone()
    }

    /// Return the narrow Blob readiness interface required by transport composition.
    pub fn storage_health_service(&self) -> StorageHealthService {
        self.storage_health_service.clone()
    }

    pub fn directory_service(&self) -> DirectoryService {
        self.directory_service.clone()
    }

    /// Return the explicit Runtime recovery interface for Directory relocations.
    pub fn directory_recovery_service(&self) -> DirectoryRecoveryService {
        self.directory_recovery_service.clone()
    }

    /// Return the explicit Runtime recovery interface for Resource relocation and deletion intents.
    pub fn resource_recovery_service(&self) -> ResourceRecoveryService {
        self.resource_recovery_service.clone()
    }

    /// Return the explicit Runtime recovery interface for content replacement intents.
    pub fn content_recovery_service(&self) -> ContentRecoveryService {
        self.content_recovery_service.clone()
    }

    pub fn asset_workflow_service(&self) -> AssetWorkflowService {
        self.asset_workflow_service.clone()
    }

    /// 返回供 Application Surface 提交上传最终化工作的窄接口。
    pub fn upload_finalization_dispatcher(&self) -> Arc<dyn UploadFinalizationDispatcher> {
        self.upload_finalizations.clone()
    }
}

#[cfg(test)]
mod tests;
