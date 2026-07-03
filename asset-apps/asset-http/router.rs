use crate::handlers;
use crate::openapi::ApiDoc;
use crate::settings::{CorsPolicy, RouterOptions};
use crate::state::HttpState;
use asset_core::port::ResourceKindRegistry;
use asset_core::service::ResourceService;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Method, StatusCode};
use axum::routing::{delete, get, post, put};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// 构建 HTTP 路由。
///
/// 该函数只负责路由注册和共享状态注入，不做运行时初始化。
#[allow(dead_code)]
pub(crate) fn build(
    service: ResourceService,
    kind_registry: Arc<dyn ResourceKindRegistry>,
) -> Router {
    build_with_options(service, kind_registry, RouterOptions::default())
}

/// 使用显式边界配置构建 HTTP 路由。
#[allow(dead_code)]
pub(crate) fn build_with_options(
    service: ResourceService,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    options: RouterOptions,
) -> Router {
    build_with_options_and_plugin_web_roots(service, kind_registry, options, Default::default())
}

/// 使用显式边界配置和插件 web 根目录构建 HTTP 路由。
pub(crate) fn build_with_options_and_plugin_web_roots(
    service: ResourceService,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    options: RouterOptions,
    plugin_web_roots: std::collections::HashMap<String, std::path::PathBuf>,
) -> Router {
    let mut router = Router::new()
        .route("/health", get(handlers::health))
        .route(
            "/plugins/{plugin_id}/{*path}",
            get(handlers::plugin_web_asset),
        )
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
        .route("/resources/{id}/preview", get(handlers::preview_resource))
        .route(
            "/resources/{id}/thumbnail",
            get(handlers::thumbnail_resource),
        )
        .route("/resources/{id}/read", get(handlers::read_resource))
        .route(
            "/resources/{id}/actions/{action}",
            post(handlers::execute_resource_action),
        );

    router = if options.enable_purge {
        router.route("/resources/{id}/purge", delete(handlers::remove_resource))
    } else {
        router.route("/resources/{id}/purge", delete(handlers::purge_disabled))
    };

    if options.enable_swagger {
        router = router
            .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()));
    }

    router
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    options.request_timeout,
                ))
                .layer(cors_layer(options.cors))
                .layer(DefaultBodyLimit::max(handlers::MAX_UPLOAD_BYTES)),
        )
        .with_state(HttpState::new_with_plugin_web_roots(
            service,
            kind_registry,
            plugin_web_roots,
        ))
}

fn cors_layer(policy: CorsPolicy) -> CorsLayer {
    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("authorization"),
        ]);

    match policy {
        CorsPolicy::None => layer,
        CorsPolicy::Any => layer.allow_origin(Any),
        CorsPolicy::Origins(origins) => layer.allow_origin(origins),
    }
}
