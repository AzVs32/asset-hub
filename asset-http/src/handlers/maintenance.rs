use super::*;
use crate::SessionStoreHealth;
use axum::extract::Extension;

/// 健康检查。
#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    security(()),
    responses(
        (status = 200, description = "服务就绪", body = HealthResponse),
        (status = 503, description = "数据库、Session 存储或对象存储不可用", body = HealthResponse)
    )
)]
pub(crate) async fn health(
    State(state): State<HttpState>,
    session_health: Option<Extension<SessionStoreHealth>>,
) -> (StatusCode, Json<HealthResponse>) {
    let session_health = session_health.map(|Extension(health)| health);
    let (database, blob_storage, session_store) = tokio::join!(
        state.service().check_repository_health(),
        state.service().check_blob_storage_health(),
        async move {
            match session_health {
                Some(health) => Some(health.check().await),
                None => None,
            }
        }
    );
    if let Err(error) = &database {
        tracing::error!(error = %error, "database readiness check failed");
    }
    if let Err(error) = &blob_storage {
        tracing::error!(error = %error, "blob storage readiness check failed");
    }
    if let Some(Err(error)) = &session_store {
        tracing::error!(error = %error, "HTTP session store readiness check failed");
    }
    let session_store_ready = session_store.as_ref().map(Result::is_ok);
    let ready = database.is_ok() && blob_storage.is_ok() && session_store_ready.unwrap_or(true);
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(HealthResponse::new(
            database.is_ok(),
            blob_storage.is_ok(),
            session_store_ready,
        )),
    )
}

/// 物理删除接口已被启动配置禁用。
pub(crate) async fn purge_disabled() -> Result<StatusCode, HttpError> {
    Err(HttpError::forbidden(
        "resource purge endpoint is disabled by --enable-purge=false",
    ))
}
