use asset_core::CoreError;
use asset_core::domain::AccessContext;
use asset_core::service::{
    ActionOrchestrator, AssetWorkflowService, AuthorizationService, ContentService,
    DirectoryService, ResourceService, SecuredActionOrchestrator, SecuredAssetWorkflowService,
    SecuredContentService, SecuredDirectoryService, SecuredResourceService, SecuredUploadService,
    UploadService, WorkspaceScope,
};
use asset_runtime::{PluginWebAssets, UploadFinalizationDispatcher};
use std::sync::Arc;

/// HTTP handler 共享状态。
///
/// Axum 会为每个请求 clone 该状态；各服务内部只 clone 端口引用，因此成本较低。
#[derive(Clone)]
pub(crate) struct HttpState {
    resources: ResourceService,
    content: ContentService,
    uploads: UploadService,
    resource_actions: ActionOrchestrator,
    directories: DirectoryService,
    asset_workflows: AssetWorkflowService,
    plugin_web_assets: Arc<PluginWebAssets>,
    authorization: AuthorizationService,
    upload_finalizations: Arc<dyn UploadFinalizationDispatcher>,
}

impl HttpState {
    pub(crate) fn new_with_plugin_web_assets(
        resources: ResourceService,
        content: ContentService,
        uploads: UploadService,
        resource_actions: ActionOrchestrator,
        directories: DirectoryService,
        asset_workflows: AssetWorkflowService,
        plugin_web_assets: PluginWebAssets,
        authorization: AuthorizationService,
        upload_finalizations: Arc<dyn UploadFinalizationDispatcher>,
    ) -> Self {
        Self {
            resources,
            content,
            uploads,
            resource_actions,
            directories,
            asset_workflows,
            plugin_web_assets: Arc::new(plugin_web_assets),
            authorization,
            upload_finalizations,
        }
    }

    pub(crate) fn secured_resources<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredResourceService<'a> {
        self.resources.secured(&self.authorization, context)
    }

    pub(crate) fn secured_directories<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredDirectoryService<'a> {
        self.directories.secured(&self.authorization, context)
    }

    pub(crate) fn secured_content<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredContentService<'a> {
        self.content.secured(&self.authorization, context)
    }

    pub(crate) fn secured_uploads<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredUploadService<'a> {
        self.uploads.secured(&self.authorization, context)
    }

    pub(crate) fn secured_resource_actions<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredActionOrchestrator<'a> {
        self.resource_actions.secured(&self.authorization, context)
    }

    pub(crate) fn secured_asset_coordination<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredAssetWorkflowService<'a> {
        self.asset_workflows.secured(&self.authorization, context)
    }

    pub(crate) fn dispatch_upload_finalization(
        &self,
        id: asset_core::domain::UploadId,
    ) -> Result<(), CoreError> {
        self.upload_finalizations.dispatch(id)
    }

    pub(crate) async fn workspace(
        &self,
        context: &AccessContext,
    ) -> Result<WorkspaceScope, CoreError> {
        self.authorization.workspace_scope(context).await
    }

    pub(crate) fn resources(&self) -> &ResourceService {
        &self.resources
    }

    pub(crate) fn resource_actions(&self) -> &ActionOrchestrator {
        &self.resource_actions
    }

    pub(crate) fn directories(&self) -> &DirectoryService {
        &self.directories
    }

    pub(crate) fn plugin_web_asset(
        &self,
        plugin_id: &str,
        path: &std::path::Path,
    ) -> Option<&Arc<[u8]>> {
        self.plugin_web_assets.get(plugin_id)?.get(path)
    }
}
