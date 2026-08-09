use crate::auth::{self, AuthBackend};
use crate::handlers;
use crate::openapi::ApiDoc;
use crate::session_store::SessionStoreHealth;
use crate::settings::{CorsPolicy, RouterOptions, SessionOptions};
use crate::state::HttpState;
use asset_core::service::ResourceService;
use asset_core::service::{AuthorizationService, UserService};
use asset_runtime::{PluginWebAssets, UploadFinalizationDispatcher};
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Method, StatusCode};
use axum::middleware;
use axum::routing::{delete, get, post};
use axum_login::AuthManagerLayerBuilder;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::{Expiry, SessionManagerLayer, cookie::SameSite, session_store::SessionStore};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// 使用显式边界配置和插件 web 根目录构建 HTTP 路由。
pub fn build_router(
    service: ResourceService,
    options: RouterOptions,
    plugin_web_assets: PluginWebAssets,
    authorization: AuthorizationService,
    upload_finalizations: Arc<dyn UploadFinalizationDispatcher>,
) -> Router {
    let mut router = Router::new()
        .route("/health", get(handlers::health))
        .route(
            "/plugins/{plugin_id}/{*path}",
            get(handlers::plugin_web_asset),
        )
        .route("/resource-kinds", get(handlers::list_resource_kinds))
        .route("/directory-kinds", get(handlers::list_directory_kinds))
        .route(
            "/directories",
            get(handlers::list_directory).post(handlers::create_directory),
        )
        .route(
            "/directories/{id}",
            get(handlers::find_directory)
                .patch(handlers::update_directory)
                .delete(handlers::delete_directory),
        )
        .route("/resources", get(handlers::list_resources))
        .route(
            "/resources/{id}",
            get(handlers::find_resource)
                .patch(handlers::update_resource)
                .delete(handlers::soft_delete_resource),
        )
        .route(
            "/resources/{id}/download",
            get(handlers::download_resource_content),
        )
        .route(
            "/resources/{id}/actions/{action}",
            post(handlers::execute_resource_action)
                .layer(DefaultBodyLimit::max(handlers::MAX_ACTION_REQUEST_BYTES)),
        );
    router = router.route(
        "/directories/{id}/actions/{action}",
        post(handlers::execute_directory_action)
            .layer(DefaultBodyLimit::max(handlers::MAX_ACTION_REQUEST_BYTES)),
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

    let upload_router = Router::new()
        .route("/uploads", post(handlers::create_upload))
        .route(
            "/uploads/{id}",
            axum::routing::patch(handlers::append_upload)
                .get(handlers::upload_status)
                .delete(handlers::abort_upload),
        )
        .route("/uploads/{id}/complete", post(handlers::complete_upload))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors_layer(options.cors.clone()))
                .layer(DefaultBodyLimit::disable()),
        );

    let resource_content_router = Router::new()
        .route(
            "/resources/{id}/content",
            get(handlers::get_resource_content).put(handlers::replace_resource_content),
        )
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors_layer(options.cors.clone()))
                .layer(DefaultBodyLimit::disable()),
        );

    let directory_download_router = Router::new()
        .route(
            "/directories/{id}/download",
            get(handlers::download_directory),
        )
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors_layer(options.cors.clone())),
        );

    router
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    options.request_timeout,
                ))
                .layer(cors_layer(options.cors)),
        )
        .merge(upload_router)
        .merge(resource_content_router)
        .merge(directory_download_router)
        .with_state(HttpState::new_with_plugin_web_assets(
            service,
            plugin_web_assets,
            authorization,
            upload_finalizations,
        ))
}

/// 为既有 API 增加由 host 提供的会话存储、登录接口和登录保护。
pub fn with_authentication<S>(
    router: Router,
    users: UserService,
    session_store: S,
    session_health: SessionStoreHealth,
    session_options: &SessionOptions,
) -> Result<Router, Box<dyn std::error::Error>>
where
    S: SessionStore + Clone,
{
    let backend = AuthBackend::new(users);
    let inactivity_seconds = i64::try_from(session_options.inactivity_timeout.as_secs())?;
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(session_options.cookie_secure)
        .with_http_only(true)
        .with_same_site(SameSite::Strict)
        .with_expiry(Expiry::OnInactivity(time::Duration::seconds(
            inactivity_seconds,
        )))
        .with_name("asset_hub_session");
    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer).build();

    let protected = router.route_layer(middleware::from_fn(auth::authorize_request));
    let public = Router::new()
        .route(
            "/auth/login",
            post(auth::login).layer(DefaultBodyLimit::max(auth::MAX_LOGIN_REQUEST_BYTES)),
        )
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/users", get(auth::list_users).post(auth::create_user))
        .route(
            "/auth/users/{id}",
            axum::routing::patch(auth::update_user_status),
        );

    Ok(protected
        .merge(public)
        .layer(auth_layer)
        .layer(axum::Extension(session_health)))
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
            HeaderName::from_static("upload-offset"),
            HeaderName::from_static("upload-checksum"),
            HeaderName::from_static("content-sha256"),
            HeaderName::from_static("if-match"),
        ])
        .expose_headers([
            HeaderName::from_static("upload-offset"),
            HeaderName::from_static("upload-length"),
        ]);

    match policy {
        CorsPolicy::None => layer,
        CorsPolicy::Origins(origins) => layer.allow_origin(origins).allow_credentials(true),
    }
}
