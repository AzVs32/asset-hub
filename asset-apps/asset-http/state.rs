use asset_core::domain::AccessContext;
use asset_core::port::ResourceKindRegistry;
use asset_core::service::{AuthorizationService, ResourceService, SecuredResourceService};
use std::sync::Arc;

/// HTTP handler 共享状态。
///
/// Axum 会为每个请求 clone 该状态；`ResourceService` 内部只 clone 端口引用，因此成本较低。
#[derive(Clone)]
pub(crate) struct HttpState {
    service: ResourceService,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    plugin_web_assets: Arc<asset_infra::PluginWebAssets>,
    authorization: AuthorizationService,
}

impl HttpState {
    pub(crate) fn new_with_plugin_web_assets(
        service: ResourceService,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        plugin_web_assets: asset_infra::PluginWebAssets,
        authorization: AuthorizationService,
    ) -> Self {
        Self {
            service,
            kind_registry,
            plugin_web_assets: Arc::new(plugin_web_assets),
            authorization,
        }
    }

    pub(crate) fn secured<'a>(&'a self, context: &'a AccessContext) -> SecuredResourceService<'a> {
        self.service.secured(&self.authorization, context)
    }

    /// 返回资源应用服务。
    pub(crate) fn service(&self) -> &ResourceService {
        &self.service
    }

    /// 返回资源类型注册表。
    pub(crate) fn kind_registry(&self) -> &dyn ResourceKindRegistry {
        self.kind_registry.as_ref()
    }

    pub(crate) fn plugin_web_asset(
        &self,
        plugin_id: &str,
        path: &std::path::Path,
    ) -> Option<&Arc<[u8]>> {
        self.plugin_web_assets.get(plugin_id)?.get(path)
    }
}
