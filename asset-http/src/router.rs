use crate::auth::{self, AuthBackend};
use crate::handlers;
use crate::openapi::ApiDoc;
use crate::session_store::SessionStoreHealth;
use crate::settings::{CorsPolicy, RouterOptions, SessionOptions};
use crate::state::{HttpComposition, HttpState};
use asset_core::service::UserService;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Method, StatusCode};
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_login::AuthManagerLayerBuilder;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::{Expiry, SessionManagerLayer, cookie::SameSite, session_store::SessionStore};
use utoipa::OpenApi;

async fn openapi_document() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

/// Build the HTTP router from one explicit composition bundle and transport policy.
pub fn build_router(composition: HttpComposition, options: RouterOptions) -> Router {
    let router = Router::new()
        .route("/health", get(handlers::health))
        .route("/api-docs/openapi.json", get(openapi_document))
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
                .delete(handlers::delete_resource),
        )
        .route(
            "/resources/{id}/download",
            get(handlers::download_resource_content),
        );

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
        .with_state(HttpState::new(composition))
}

/// 为 API 接入调用方提供的会话存储，并增加登录接口和登录保护。
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
            HeaderName::from_static("idempotency-key"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{HeaderValue, Request, header};
    use tower::ServiceExt;

    #[tokio::test]
    async fn idempotency_key_is_allowed_for_cross_origin_preflight() {
        let router = Router::new()
            .route("/uploads", post(|| async { StatusCode::CREATED }))
            .layer(cors_layer(CorsPolicy::Origins(vec![
                HeaderValue::from_static("https://example.test"),
            ])));
        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/uploads")
            .header(header::ORIGIN, "https://example.test")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, Method::POST.as_str())
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "idempotency-key,content-type",
            )
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let allowed_headers = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            allowed_headers
                .split(',')
                .map(str::trim)
                .any(|name| name.eq_ignore_ascii_case("idempotency-key"))
        );
    }
}
