use crate::handlers;
use crate::openapi::ApiDoc;
use crate::state::HttpState;
use asset_core::port::ResourceKindRegistry;
use asset_core::service::ResourceService;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post, put};
use std::sync::Arc;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// 构建 HTTP 路由。
///
/// 该函数只负责路由注册和共享状态注入，不做运行时初始化。
pub(crate) fn build(
    service: ResourceService,
    kind_registry: Arc<dyn ResourceKindRegistry>,
) -> Router {
    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .route("/health", get(handlers::health))
        .route("/resource-kinds", get(handlers::list_resource_kinds))
        .route(
            "/resources",
            get(handlers::list_resources).post(handlers::create_resource),
        )
        .route(
            "/resources/content",
            post(handlers::upload_resource_content),
        )
        .route(
            "/resources/content/stream",
            put(handlers::upload_resource_content_stream),
        )
        .route(
            "/resources/{id}",
            get(handlers::find_resource)
                .patch(handlers::update_resource)
                .delete(handlers::soft_delete_resource),
        )
        .route(
            "/resources/{id}/content",
            get(handlers::get_resource_content),
        )
        .route("/resources/{id}/read", get(handlers::read_resource))
        .route("/resources/{id}/purge", delete(handlers::remove_resource))
        .layer(DefaultBodyLimit::max(handlers::MAX_UPLOAD_BYTES))
        .with_state(HttpState::new(service, kind_registry))
}
