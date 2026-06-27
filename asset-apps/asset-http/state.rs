use asset_core::service::ResourceService;

/// HTTP handler 共享状态。
///
/// Axum 会为每个请求 clone 该状态；`ResourceService` 内部只 clone 端口引用，因此成本较低。
#[derive(Clone)]
pub(crate) struct HttpState {
    service: ResourceService,
}

impl HttpState {
    /// 创建 HTTP 共享状态。
    pub(crate) fn new(service: ResourceService) -> Self {
        Self { service }
    }

    /// 返回资源应用服务。
    pub(crate) fn service(&self) -> &ResourceService {
        &self.service
    }
}
