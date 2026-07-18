use crate::auth::{self, AuthBackend};
use crate::handlers;
use crate::openapi::ApiDoc;
use crate::settings::{CorsPolicy, RouterOptions, SessionOptions};
use crate::state::HttpState;
use asset_core::port::ResourceKindRegistry;
use asset_core::service::ResourceService;
use asset_core::service::{AuthorizationService, UserService};
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Method, StatusCode};
use axum::middleware;
use axum::routing::{delete, get, post, put};
use axum_login::AuthManagerLayerBuilder;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::{
    Expiry, SessionManagerLayer, cookie::SameSite, session_store::ExpiredDeletion,
};
use tower_sessions_sqlx_store::{
    SqliteStore,
    sqlx::sqlite::{
        SqliteConnectOptions as SessionConnectOptions, SqlitePoolOptions as SessionPoolOptions,
    },
};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// 使用显式边界配置和插件 web 根目录构建 HTTP 路由。
pub(crate) fn build_with_options_and_plugin_web_assets(
    service: ResourceService,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    options: RouterOptions,
    plugin_web_assets: asset_infra::PluginWebAssets,
    authorization: AuthorizationService,
) -> Router {
    let mut router = Router::new()
        .route("/health", get(handlers::health))
        .route(
            "/plugins/{plugin_id}/{*path}",
            get(handlers::plugin_web_asset),
        )
        .route("/resource-kinds", get(handlers::list_resource_kinds))
        .route(
            "/directories",
            get(handlers::list_directory).post(handlers::create_directory),
        )
        .route("/scan", post(handlers::scan_storage))
        .route(
            "/resources",
            get(handlers::list_resources).post(handlers::create_resource),
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
            post(handlers::execute_resource_action)
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
        .route(
            "/resources/content/stream",
            put(handlers::upload_resource_content_stream),
        )
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors_layer(options.cors.clone()))
                .layer(DefaultBodyLimit::max(handlers::MAX_UPLOAD_BYTES)),
        );

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
        .merge(upload_router)
        .with_state(HttpState::new_with_plugin_web_assets(
            service,
            kind_registry,
            plugin_web_assets,
            authorization,
        ))
}

/// 为既有 API 增加 SQLite 会话、登录接口和登录保护。
pub(crate) async fn with_authentication(
    router: Router,
    users: UserService,
    authorization: AuthorizationService,
    sqlite_path: &std::path::Path,
    bootstrap_admin: Option<(&str, &str)>,
    session_options: &SessionOptions,
) -> Result<Router, Box<dyn std::error::Error>> {
    let session_connect_options = SessionConnectOptions::new()
        .filename(sqlite_path)
        .create_if_missing(true);
    let session_pool = SessionPoolOptions::new()
        .max_connections(2)
        .connect_with(session_connect_options)
        .await?;
    let audit = crate::audit::SecurityAuditLog::new(session_pool.clone());
    let backend = AuthBackend::new(users, authorization, audit);
    backend.initialize(bootstrap_admin).await?;
    let session_store = SqliteStore::new(session_pool);
    session_store.migrate().await?;
    let _session_cleanup = tokio::spawn(
        session_store
            .clone()
            .continuously_delete_expired(std::time::Duration::from_secs(60 * 60)),
    );
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
        .route("/auth/audit-events", get(auth::list_security_audit_events))
        .route("/auth/users", get(auth::list_users).post(auth::create_user))
        .route(
            "/auth/users/{id}",
            axum::routing::patch(auth::update_user_status),
        );

    Ok(protected
        .merge(public)
        .layer(middleware::from_fn(auth::audit_request))
        .layer(auth_layer))
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
        CorsPolicy::Origins(origins) => layer.allow_origin(origins).allow_credentials(true),
    }
}
