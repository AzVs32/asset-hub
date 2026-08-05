//! 资源应用服务门面。
//!
//! 本模块只负责装配 Host Port、暴露可信维护入口，并把具体用例路由到内部子服务。
//! 公开输入/输出位于 `contract`，资源生命周期、内容和动作分别由对应模块编排。

use crate::CoreError;
use crate::domain::{
    Resource, ResourceActionDefinition, ResourceActionPolicy, ResourceContentEditPolicy,
    ResourceKind, StorageKey,
};
use crate::port::{
    BlobStorage, DirectoryLocation, ResourceActionExecutor, ResourceActionRegistry,
    ResourceContentReplacementRepository, ResourceKindRegistry, ResourceQuery, ResourceRepository,
    StorageScanner, UploadSessionRepository,
};
use crate::service::DirectoryService;
use std::sync::Arc;

mod action;
mod command;
mod content;
mod contract;
mod reconciliation;
mod secured;
mod storage_key_locks;
mod upload;
mod upload_locks;

use action::ResourceActionService;
use command::ResourceCommandService;
use content::ResourceContentService;
pub use contract::{
    CreateUpload, DirectoryArchiveManifest, DirectoryArchiveResource, ExecuteResourceAction,
    ReplaceResourceContent, ResourceActions, ResourceContentStream, UpdateResource,
};
pub use reconciliation::StorageReconciliationReport;
use reconciliation::StorageReconciliationService;
pub use secured::SecuredResourceService;
use storage_key_locks::StorageKeyLocks;
use upload::ResourceUploadService;
use upload_locks::UploadLocks;

/// 资源应用服务。
///
/// 外部用户入口应通过 [`ResourceService::secured`] 获取带授权上下文的门面；未授权门面只
/// 暴露健康检查和存储协调等可信维护入口。具体业务编排分布在 command、content、action
///、reconciliation 和 secured 子服务中。
#[derive(Clone)]
pub struct ResourceService {
    repository: Arc<dyn ResourceRepository>,
    query: Arc<dyn ResourceQuery>,
    blob_storage: Arc<dyn BlobStorage>,
    directories: DirectoryService,
    storage_scanner: Arc<dyn StorageScanner>,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    action_ports: Option<ResourceActionPorts>,
    resource_action_policy: Arc<ResourceActionPolicy>,
    resource_content_edit_policy: Arc<ResourceContentEditPolicy>,
    content_replacements: Arc<dyn ResourceContentReplacementRepository>,
    storage_key_locks: Arc<StorageKeyLocks>,
    upload_sessions: Arc<dyn UploadSessionRepository>,
    upload_locks: Arc<UploadLocks>,
}

#[derive(Clone)]
struct ResourceActionPorts {
    registry: Arc<dyn ResourceActionRegistry>,
    executor: Arc<dyn ResourceActionExecutor>,
}

/// `ResourceService` 所需的 Host Port 装配。
///
/// 写模型、读模型、Blob、扫描器和 kind 注册表是必选端口；动作注册表与执行器必须成对
/// 注入，避免出现只有动作声明或只有执行器的半配置状态。目录能力通过已经装配完成的
/// [`DirectoryService`] 注入 `ResourceService`，确保所有应用服务共享同一并发边界。
pub struct ResourceServicePorts {
    repository: Arc<dyn ResourceRepository>,
    query: Arc<dyn ResourceQuery>,
    blob_storage: Arc<dyn BlobStorage>,
    storage_scanner: Arc<dyn StorageScanner>,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    upload_sessions: Arc<dyn UploadSessionRepository>,
    content_replacements: Arc<dyn ResourceContentReplacementRepository>,
    action_ports: Option<ResourceActionPorts>,
}

impl ResourceServicePorts {
    pub fn new(
        repository: Arc<dyn ResourceRepository>,
        query: Arc<dyn ResourceQuery>,
        blob_storage: Arc<dyn BlobStorage>,
        storage_scanner: Arc<dyn StorageScanner>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        upload_sessions: Arc<dyn UploadSessionRepository>,
        content_replacements: Arc<dyn ResourceContentReplacementRepository>,
    ) -> Self {
        Self {
            repository,
            query,
            blob_storage,
            storage_scanner,
            kind_registry,
            upload_sessions,
            content_replacements,
            action_ports: None,
        }
    }

    pub fn with_actions(
        mut self,
        registry: Arc<dyn ResourceActionRegistry>,
        executor: Arc<dyn ResourceActionExecutor>,
    ) -> Self {
        self.action_ports = Some(ResourceActionPorts { registry, executor });
        self
    }
}

impl ResourceService {
    /// 创建资源应用服务。
    pub fn new(
        ports: ResourceServicePorts,
        directories: DirectoryService,
        resource_action_policy: Arc<ResourceActionPolicy>,
        resource_content_edit_policy: Arc<ResourceContentEditPolicy>,
    ) -> Self {
        let ResourceServicePorts {
            repository,
            query,
            blob_storage,
            storage_scanner,
            kind_registry,
            upload_sessions,
            content_replacements,
            action_ports,
        } = ports;
        Self {
            repository,
            query,
            blob_storage,
            directories,
            storage_scanner,
            kind_registry,
            action_ports,
            resource_action_policy,
            resource_content_edit_policy,
            content_replacements,
            storage_key_locks: Arc::new(StorageKeyLocks::default()),
            upload_sessions,
            upload_locks: Arc::new(UploadLocks::default()),
        }
    }

    pub fn directory_service(&self) -> &DirectoryService {
        &self.directories
    }

    fn commands(&self) -> ResourceCommandService<'_> {
        ResourceCommandService::new(self)
    }

    fn content(&self) -> ResourceContentService<'_> {
        ResourceContentService::new(self)
    }

    fn uploads(&self) -> ResourceUploadService<'_> {
        ResourceUploadService::new(self)
    }

    fn actions(&self) -> ResourceActionService<'_> {
        ResourceActionService::new(self)
    }

    fn reconciliation(&self) -> StorageReconciliationService<'_> {
        StorageReconciliationService::new(self)
    }

    /// 将资源用例绑定到访问主体。HTTP、CLI、TUI 等非可信入口应通过该门面执行操作。
    pub fn secured<'a>(
        &'a self,
        authorization: &'a crate::service::AuthorizationService,
        context: &'a crate::domain::AccessContext,
    ) -> SecuredResourceService<'a> {
        SecuredResourceService::new(self, authorization, context)
    }

    pub async fn check_repository_health(&self) -> Result<(), CoreError> {
        self.repository.health_check().await
    }

    pub async fn check_blob_storage_health(&self) -> Result<(), CoreError> {
        self.blob_storage.health_check().await
    }

    /// 返回需要由 Runtime 恢复调度的上传 finalization。
    pub async fn pending_upload_finalizations(
        &self,
    ) -> Result<Vec<crate::domain::UploadId>, CoreError> {
        self.upload_sessions.list_finalizing().await
    }

    /// Recover content replacements interrupted between Blob publication and Resource persistence.
    pub async fn resume_content_replacements(&self) -> Result<usize, CoreError> {
        self.content().resume_pending_replacements().await
    }

    /// 执行单个已经进入 `Finalizing` 状态的上传；任务生命周期由 Runtime 持有。
    pub async fn finalize_upload(
        &self,
        id: &crate::domain::UploadId,
    ) -> Result<Resource, CoreError> {
        self.uploads().finalize(id).await
    }

    pub async fn locate_resource_directory(
        &self,
        resource: &Resource,
    ) -> Result<DirectoryLocation, CoreError> {
        self.directories
            .locate_by_id(&resource.directory_id())
            .await
    }

    #[cfg(test)]
    async fn storage_key(&self, resource: &Resource) -> Result<StorageKey, CoreError> {
        let directory = self.locate_resource_directory(resource).await?;
        StorageKey::from_resource_path(directory.path(), resource.name()).map_err(Into::into)
    }

    /// 使用大小与修改时间增量协调对象存储。
    pub async fn reconcile_storage(&self) -> Result<StorageReconciliationReport, CoreError> {
        self.reconciliation().reconcile_storage(false).await
    }

    /// 启动恢复：空仓储先按对象元数据建立 pending Resource，再由调用方后台校验。
    pub async fn reconcile_storage_on_startup(
        &self,
    ) -> Result<StorageReconciliationReport, CoreError> {
        self.reconciliation().reconcile_storage_on_startup().await
    }

    /// 完整读取全部对象并重新计算校验和。
    pub async fn scan_resources(&self) -> Result<StorageReconciliationReport, CoreError> {
        self.reconciliation().reconcile_storage(true).await
    }

    /// 协调一组发生变化的对象路径。
    pub async fn reconcile_storage_keys(&self, keys: &[StorageKey]) -> Result<(), CoreError> {
        self.reconciliation().reconcile_storage_keys(keys).await
    }

    /// 协调文件系统确认的单文件重命名，成功时保留 Resource ID 和元数据。
    pub async fn reconcile_storage_rename(
        &self,
        from: &StorageKey,
        to: &StorageKey,
    ) -> Result<(), CoreError> {
        self.reconciliation()
            .reconcile_storage_rename(from, to)
            .await
    }

    /// 计算已授权资源当前可展示的动作，不执行插件或产生写副作用。
    pub fn describe_resource_actions(
        &self,
        resource: &Resource,
    ) -> Result<ResourceActions, CoreError> {
        self.actions().describe_resource_actions(resource)
    }

    fn validate_registered_kind(
        &self,
        kind: Option<ResourceKind>,
    ) -> Result<ResourceKind, CoreError> {
        let kind = kind.unwrap_or_default();
        self.ensure_kind_registered(&kind)?;
        Ok(kind)
    }

    fn validate_content_kind(&self, kind: Option<ResourceKind>) -> Result<ResourceKind, CoreError> {
        let kind = self.validate_registered_kind(kind)?;
        let definition = self.require_kind_definition(&kind)?;
        if !definition.supports_content() {
            return Err(CoreError::unsupported(
                "resource kind for content upload",
                kind.to_string(),
            ));
        }
        Ok(kind)
    }

    fn resolve_content_kind(
        &self,
        kind: Option<ResourceKind>,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> Result<ResourceKind, CoreError> {
        let kind = kind.or_else(|| {
            self.kind_registry
                .detect_content_kind(mime_type, storage_key)
        });
        self.validate_content_kind(kind)
    }

    fn ensure_kind_registered(&self, kind: &ResourceKind) -> Result<(), CoreError> {
        if self.kind_registry.supports(kind) {
            return Ok(());
        }
        Err(CoreError::unsupported("resource kind", kind.to_string()))
    }

    fn require_kind_definition(
        &self,
        kind: &ResourceKind,
    ) -> Result<&crate::port::ResourceKindDefinition, CoreError> {
        self.kind_registry.get(kind).ok_or_else(|| {
            CoreError::invariant(format!(
                "persisted resource kind `{kind}` is not registered"
            ))
        })
    }

    fn actions_for_resource_kind(&self, kind: &ResourceKind) -> Vec<ResourceActionDefinition> {
        let lineage = self.kind_registry.lineage(kind);
        self.action_ports
            .as_ref()
            .map(|ports| ports.registry.actions_for_kinds(&lineage))
            .unwrap_or_default()
    }

    fn action_candidates_for_resource_kind(
        &self,
        kind: &ResourceKind,
    ) -> Vec<ResourceActionDefinition> {
        let lineage = self.kind_registry.lineage(kind);
        self.action_ports
            .as_ref()
            .map(|ports| ports.registry.action_candidates_for_kinds(&lineage))
            .unwrap_or_default()
    }

    fn available_actions_for_resource(&self, resource: &Resource) -> Vec<ResourceActionDefinition> {
        let applicable = self
            .action_candidates_for_resource_kind(resource.kind())
            .into_iter()
            .filter(|action| resource.content().is_some() || !action.requirements().content)
            .filter(|action| self.action_matches_resource(action, resource))
            .filter(|action| {
                let is_text_edit = action
                    .provides()
                    .is_some_and(|capability| capability.as_str() == "text_edit");
                !is_text_edit
                    || resource.content().is_some_and(|content| {
                        content.size() <= self.resource_content_edit_policy.max_text_bytes()
                    })
            })
            .collect::<Vec<_>>();
        self.action_ports
            .as_ref()
            .map(|ports| ports.registry.resolve_capability_providers(applicable))
            .unwrap_or_default()
    }

    /// 返回指定 kind 及其祖先谱系适用的动作定义。
    pub fn describe_kind_actions(&self, kind: &ResourceKind) -> Vec<ResourceActionDefinition> {
        self.actions_for_resource_kind(kind)
    }

    fn action_matches_resource(
        &self,
        action: &ResourceActionDefinition,
        resource: &Resource,
    ) -> bool {
        let content = resource.content();
        self.kind_registry
            .lineage(resource.kind())
            .iter()
            .any(|kind| {
                action.matches_resource(
                    kind.as_str(),
                    content.and_then(|content| content.mime_type()),
                    content.map(|_| resource.name()),
                )
            })
    }
}

#[cfg(test)]
mod tests;
