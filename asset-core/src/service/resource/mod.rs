//! 资源应用服务门面。
//!
//! 本模块只负责装配 Host Port、暴露可信维护入口，并把具体用例路由到内部子服务。
//! 公开输入/输出位于 `contract`，资源生命周期、内容、动作和预览分别由对应模块编排。

use crate::CoreError;
use crate::domain::{Resource, ResourceKind, StorageKey};
use crate::port::{
    BlobStorage, DirectoryRepository, DirectoryStorage, ResourceActionExecutor,
    ResourceActionRegistry, ResourceKindRegistry, ResourceQuery, ResourceRepository,
    StorageScanner,
};
use crate::service::DirectoryService;
use asset_plugin_api::{PluginExecutionPolicy, ResourceActionDefinition};
use std::sync::Arc;

mod action;
mod command;
mod content;
mod contract;
mod preview;
mod reconciliation;
mod secured;

use action::ResourceActionService;
use command::ResourceCommandService;
use content::ResourceContentService;
#[cfg(test)]
use contract::ResourcePreview;
pub use contract::{
    CreateResource, ExecuteResourceAction, ReadableResource, ResourceActions,
    ResourceContentCommand, ResourceContentStream, ResourcePreviewStream, ResourceThumbnail,
    UpdateResource, UploadResourceContentStream,
};
use preview::ResourcePreviewService;
pub use reconciliation::StorageReconciliationReport;
use reconciliation::StorageReconciliationService;
pub use secured::SecuredResourceService;

/// 资源应用服务。
///
/// 外部用户入口应通过 [`ResourceService::secured`] 获取带授权上下文的门面；未授权门面只
/// 暴露健康检查和存储协调等可信维护入口。具体业务编排分布在 command、content、action
///、preview、reconciliation 和 secured 子服务中。
#[derive(Clone)]
pub struct ResourceService {
    repository: Arc<dyn ResourceRepository>,
    query: Arc<dyn ResourceQuery>,
    blob_storage: Arc<dyn BlobStorage>,
    directories: DirectoryService,
    storage_scanner: Arc<dyn StorageScanner>,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    action_ports: Option<ResourceActionPorts>,
    plugin_execution_policy: Arc<PluginExecutionPolicy>,
}

#[derive(Clone)]
struct ResourceActionPorts {
    registry: Arc<dyn ResourceActionRegistry>,
    executor: Arc<dyn ResourceActionExecutor>,
}

/// `ResourceService` 所需的 Host Port 装配。
///
/// 写模型、读模型、Blob、目录聚合仓储、物理目录、扫描器和 kind 注册表是必选端口；
/// 动作注册表与执行器必须成对注入，避免出现只有动作声明或只有执行器的半配置状态。
pub struct ResourceServicePorts {
    repository: Arc<dyn ResourceRepository>,
    query: Arc<dyn ResourceQuery>,
    blob_storage: Arc<dyn BlobStorage>,
    directory_repository: Arc<dyn DirectoryRepository>,
    directory_storage: Arc<dyn DirectoryStorage>,
    storage_scanner: Arc<dyn StorageScanner>,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    action_ports: Option<ResourceActionPorts>,
}

impl ResourceServicePorts {
    pub fn new(
        repository: Arc<dyn ResourceRepository>,
        query: Arc<dyn ResourceQuery>,
        blob_storage: Arc<dyn BlobStorage>,
        directory_repository: Arc<dyn DirectoryRepository>,
        directory_storage: Arc<dyn DirectoryStorage>,
        storage_scanner: Arc<dyn StorageScanner>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
    ) -> Self {
        Self {
            repository,
            query,
            blob_storage,
            directory_repository,
            directory_storage,
            storage_scanner,
            kind_registry,
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
        plugin_execution_policy: Arc<PluginExecutionPolicy>,
    ) -> Self {
        let ResourceServicePorts {
            repository,
            query,
            blob_storage,
            directory_repository,
            directory_storage,
            storage_scanner,
            kind_registry,
            action_ports,
        } = ports;
        Self {
            repository,
            query,
            blob_storage,
            directories: DirectoryService::new(directory_repository, directory_storage),
            storage_scanner,
            kind_registry,
            action_ports,
            plugin_execution_policy,
        }
    }

    fn commands(&self) -> ResourceCommandService<'_> {
        ResourceCommandService::new(self)
    }

    fn content(&self) -> ResourceContentService<'_> {
        ResourceContentService::new(self)
    }

    fn actions(&self) -> ResourceActionService<'_> {
        ResourceActionService::new(self)
    }

    fn previews(&self) -> ResourcePreviewService<'_> {
        ResourcePreviewService::new(self)
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

    /// 使用大小与修改时间增量协调对象存储。
    pub async fn reconcile_storage(&self) -> Result<StorageReconciliationReport, CoreError> {
        self.reconciliation().reconcile_storage(false).await
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
            return Err(CoreError::configuration(format!(
                "resource kind `{kind}` does not support content upload"
            )));
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
        Err(CoreError::configuration(format!(
            "unsupported resource kind `{kind}`"
        )))
    }

    fn require_kind_definition(
        &self,
        kind: &ResourceKind,
    ) -> Result<&crate::port::ResourceKindDefinition, CoreError> {
        self.kind_registry
            .get(kind)
            .ok_or_else(|| CoreError::configuration(format!("unsupported resource kind `{kind}`")))
    }

    fn actions_for_resource_kind(&self, kind: &ResourceKind) -> Vec<ResourceActionDefinition> {
        let lineage = self.kind_registry.lineage(kind);
        self.action_ports
            .as_ref()
            .map(|ports| ports.registry.actions_for_kinds(&lineage))
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
                    content
                        .map(|_| resource.storage_key())
                        .as_ref()
                        .map(StorageKey::as_str),
                )
            })
    }
}

#[cfg(test)]
mod tests;
