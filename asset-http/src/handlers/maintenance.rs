use super::*;

/// 健康检查。
#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    security(()),
    responses(
        (status = 200, description = "服务就绪", body = HealthResponse),
        (status = 503, description = "数据库或对象存储不可用", body = HealthResponse)
    )
)]
pub(crate) async fn health(State(state): State<HttpState>) -> (StatusCode, Json<HealthResponse>) {
    let (database, blob_storage) = tokio::join!(
        state.service().check_repository_health(),
        state.service().check_blob_storage_health()
    );
    if let Err(error) = &database {
        tracing::error!(error = %error, "database readiness check failed");
    }
    if let Err(error) = &blob_storage {
        tracing::error!(error = %error, "blob storage readiness check failed");
    }
    let ready = database.is_ok() && blob_storage.is_ok();
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(HealthResponse::new(database.is_ok(), blob_storage.is_ok())),
    )
}

/// 物理删除接口已被启动配置禁用。
pub(crate) async fn purge_disabled() -> Result<StatusCode, HttpError> {
    Err(HttpError::forbidden(
        "resource purge endpoint is disabled by ASSET_HTTP_ENABLE_PURGE",
    ))
}
