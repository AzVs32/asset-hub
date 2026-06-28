use asset_core::port::ResourceKindRegistry;
use asset_core::service::ResourceService;
use std::sync::Arc;

/// HTTP handler 共享状态。
///
/// Axum 会为每个请求 clone 该状态；`ResourceService` 内部只 clone 端口引用，因此成本较低。
#[derive(Clone)]
pub(crate) struct HttpState {
    service: ResourceService,
    kind_registry: Arc<dyn ResourceKindRegistry>,
}

impl HttpState {
    /// 创建 HTTP 共享状态。
    pub(crate) fn new(
        service: ResourceService,
        kind_registry: Arc<dyn ResourceKindRegistry>,
    ) -> Self {
        Self {
            service,
            kind_registry,
        }
    }

    /// 返回资源应用服务。
    pub(crate) fn service(&self) -> &ResourceService {
        &self.service
    }

    /// 返回资源类型注册表。
    pub(crate) fn kind_registry(&self) -> &dyn ResourceKindRegistry {
        self.kind_registry.as_ref()
    }
}
