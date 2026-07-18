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

/// 扫描本地对象存储目录并补齐资源数据库。
#[utoipa::path(
    post,
    path = "/scan",
    tag = "resources",
    request_body = ScanStorageRequest,
    responses(
        (status = 200, description = "扫描结果", body = ScanStorageResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn scan_storage(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    payload: Option<Json<ScanStorageRequest>>,
) -> Result<Json<ScanStorageResponse>, HttpError> {
    let payload = payload.map(|Json(payload)| payload).unwrap_or_default();
    let result = state
        .secured(&access.0)
        .scan_storage(ScanStorage::new(payload.prefix))
        .await?;
    let imported = result
        .resources
        .iter()
        .map(|resource| resource_response(state.service(), resource))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(ScanStorageResponse {
        scanned_directory: result.scanned_prefix,
        scanned: result.scanned,
        imported: imported.len() as u64,
        skipped: result.skipped,
        errors: result
            .errors
            .into_iter()
            .map(|error| ScanStorageErrorResponse {
                key: error.key,
                error: error.error,
            })
            .collect(),
        resources: imported,
    }))
}

/// 物理删除接口已被启动配置禁用。
pub(crate) async fn purge_disabled() -> Result<StatusCode, HttpError> {
    Err(HttpError::forbidden(
        "resource purge endpoint is disabled by ASSET_HTTP_ENABLE_PURGE",
    ))
}
