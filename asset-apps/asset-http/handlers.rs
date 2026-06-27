use crate::dto::{
    BinaryContent, CreateResourceRequest, HealthResponse, ResourceMetadataRequest,
    ResourceResponse, UploadResourceContentRequest, UploadResourceContentStreamQuery,
};
use crate::error::HttpError;
use crate::state::HttpState;
use asset_core::domain::{Checksum, ResourceId, ResourceKind, ResourceStatus, StorageKey};
use asset_core::service::{CreateResource, UploadResourceContent, UploadResourceContentStream};
use asset_core::{CoreError, port::BlobByteStream};
use axum::Json;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use futures_util::StreamExt;
use std::str::FromStr;

/// 健康检查。
#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    responses(
        (status = 200, description = "服务正常", body = HealthResponse)
    )
)]
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse::ok())
}

/// 创建纯元数据资源。
#[utoipa::path(
    post,
    path = "/resources",
    tag = "resources",
    request_body = CreateResourceRequest,
    responses(
        (status = 201, description = "资源已创建", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn create_resource(
    State(state): State<HttpState>,
    payload: Result<Json<CreateResourceRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ResourceResponse>), HttpError> {
    let payload = parse_json_payload(payload)?;
    let command = apply_common_resource_fields(
        CreateResource::new(payload.name),
        payload.kind,
        payload.status,
        payload.metadata,
    )?;
    let resource = state.service().create_resource(command).await?;

    Ok((StatusCode::CREATED, Json(ResourceResponse::from(&resource))))
}

/// 上传内容并创建资源。
#[utoipa::path(
    post,
    path = "/resources/content",
    tag = "resources",
    request_body = UploadResourceContentRequest,
    responses(
        (status = 201, description = "资源内容已上传并创建资源", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn upload_resource_content(
    State(state): State<HttpState>,
    payload: Result<Json<UploadResourceContentRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ResourceResponse>), HttpError> {
    let payload = parse_json_payload(payload)?;
    let storage_key = StorageKey::new(payload.storage_key)?;
    let data = BASE64_STANDARD
        .decode(payload.data_base64.as_bytes())
        .map(Bytes::from)
        .map_err(|error| HttpError::bad_request(format!("invalid base64 content: {error}")))?;

    let mut command = UploadResourceContent::new(payload.name, storage_key, data);
    command = apply_common_upload_fields(command, payload.kind, payload.status, payload.metadata)?;

    if let Some(mime_type) = payload.mime_type {
        command = command.with_mime_type(mime_type);
    }

    if let Some(original_filename) = payload.original_filename {
        command = command.with_original_filename(original_filename);
    }

    if let Some(sha256) = payload.sha256 {
        command = command.with_checksum(Checksum::sha256(sha256)?);
    }

    let resource = state.service().upload_resource_content(command).await?;

    Ok((StatusCode::CREATED, Json(ResourceResponse::from(&resource))))
}

/// 流式上传内容并创建资源。
///
/// 请求体必须是原始二进制流。资源名称、存储键等元信息从 query 参数读取，MIME 类型
/// 优先使用请求的 `Content-Type` header。
#[utoipa::path(
    put,
    path = "/resources/content/stream",
    tag = "resources",
    params(UploadResourceContentStreamQuery),
    request_body(
        content = inline(BinaryContent),
        content_type = "application/octet-stream",
        description = "原始二进制内容流"
    ),
    responses(
        (status = 201, description = "资源内容已流式上传并创建资源", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn upload_resource_content_stream(
    State(state): State<HttpState>,
    Query(query): Query<UploadResourceContentStreamQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<ResourceResponse>), HttpError> {
    let storage_key = StorageKey::new(query.storage_key)?;
    let data: BlobByteStream = Box::pin(
        body.into_data_stream()
            .map(|chunk| chunk.map_err(|error| CoreError::storage("http.request_body", error))),
    );

    let mut command = UploadResourceContentStream::new(query.name, storage_key, data);
    command = apply_common_stream_fields(command, query.kind, query.status, query.metadata_json)?;

    if let Some(mime_type) = content_type(&headers)? {
        command = command.with_mime_type(mime_type);
    }

    if let Some(original_filename) = query.original_filename {
        command = command.with_original_filename(original_filename);
    }

    if let Some(sha256) = query.sha256 {
        command = command.with_checksum(Checksum::sha256(sha256)?);
    }

    let resource = state
        .service()
        .upload_resource_content_stream(command)
        .await?;

    Ok((StatusCode::CREATED, Json(ResourceResponse::from(&resource))))
}

/// 按 ID 查询资源。
#[utoipa::path(
    get,
    path = "/resources/{id}",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源详情", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn find_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;

    match state.service().find_resource(&id).await? {
        Some(resource) => Ok(Json(ResourceResponse::from(&resource))),
        None => Err(HttpError::not_found(format!("resource `{id}` not found"))),
    }
}

/// 读取资源内容。
#[utoipa::path(
    get,
    path = "/resources/{id}/content",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源原始内容", content_type = "application/octet-stream", body = BinaryContent),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源内容不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn get_resource_content(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_resource_id(&id)?;

    match state.service().get_resource_content(&id).await? {
        Some(content) => Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            content,
        )
            .into_response()),
        None => Err(HttpError::not_found(format!(
            "resource content `{id}` not found"
        ))),
    }
}

/// 软删除资源。
#[utoipa::path(
    delete,
    path = "/resources/{id}",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源已软删除", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn soft_delete_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;

    match state.service().soft_delete_resource(&id).await? {
        Some(resource) => Ok(Json(ResourceResponse::from(&resource))),
        None => Err(HttpError::not_found(format!("resource `{id}` not found"))),
    }
}

/// 物理移除资源和对象内容。
#[utoipa::path(
    delete,
    path = "/resources/{id}/purge",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 204, description = "资源和对象内容已物理移除"),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn remove_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpError> {
    let id = parse_resource_id(&id)?;

    if state.service().remove_resource(&id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(HttpError::not_found(format!("resource `{id}` not found")))
    }
}

fn apply_common_resource_fields(
    mut command: CreateResource,
    kind: Option<String>,
    status: Option<String>,
    metadata: Option<ResourceMetadataRequest>,
) -> Result<CreateResource, HttpError> {
    if let Some(kind) = kind {
        command = command.with_kind(ResourceKind::try_new(kind)?);
    }

    if let Some(status) = status {
        command = command.with_status(parse_status(&status)?);
    }

    if let Some(metadata) = metadata {
        command = command.with_metadata(metadata.into_domain()?);
    }

    Ok(command)
}

fn apply_common_upload_fields(
    mut command: UploadResourceContent,
    kind: Option<String>,
    status: Option<String>,
    metadata: Option<ResourceMetadataRequest>,
) -> Result<UploadResourceContent, HttpError> {
    if let Some(kind) = kind {
        command = command.with_kind(ResourceKind::try_new(kind)?);
    }

    if let Some(status) = status {
        command = command.with_status(parse_status(&status)?);
    }

    if let Some(metadata) = metadata {
        command = command.with_metadata(metadata.into_domain()?);
    }

    Ok(command)
}

fn apply_common_stream_fields(
    mut command: UploadResourceContentStream,
    kind: Option<String>,
    status: Option<String>,
    metadata_json: Option<String>,
) -> Result<UploadResourceContentStream, HttpError> {
    if let Some(kind) = kind {
        command = command.with_kind(ResourceKind::try_new(kind)?);
    }

    if let Some(status) = status {
        command = command.with_status(parse_status(&status)?);
    }

    if let Some(metadata_json) = metadata_json {
        let metadata = serde_json::from_str::<ResourceMetadataRequest>(&metadata_json)
            .map_err(|error| HttpError::bad_request(format!("invalid metadata_json: {error}")))?;
        command = command.with_metadata(metadata.into_domain()?);
    }

    Ok(command)
}

fn content_type(headers: &HeaderMap) -> Result<Option<String>, HttpError> {
    headers
        .get(header::CONTENT_TYPE)
        .map(|value| {
            value
                .to_str()
                .map(|value| value.to_string())
                .map_err(|error| HttpError::bad_request(format!("invalid content-type: {error}")))
        })
        .transpose()
}

fn parse_status(value: &str) -> Result<ResourceStatus, HttpError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "active" => Ok(ResourceStatus::Active),
        "archived" => Ok(ResourceStatus::Archived),
        _ => Err(HttpError::bad_request(format!(
            "invalid resource status `{value}`"
        ))),
    }
}

fn parse_resource_id(value: &str) -> Result<ResourceId, HttpError> {
    ResourceId::from_str(value).map_err(|error| HttpError::bad_request(error.to_string()))
}

fn parse_json_payload<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, HttpError> {
    payload
        .map(|Json(payload)| payload)
        .map_err(|error| HttpError::bad_request(format!("invalid JSON request body: {error}")))
}
