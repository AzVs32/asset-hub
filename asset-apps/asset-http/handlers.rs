use crate::dto::{
    BinaryContent, CreateResourceRequest, ExecuteResourceActionRequest, HealthResponse,
    ListResourcesQuery, ResourceActionOutputResponse, ResourceKindResponse, ResourceKindsResponse,
    ResourceMetadataRequest, ResourcePageResponse, ResourceReadResponse, ResourceResponse,
    UpdateResourceRequest, UploadResourceContentRequest, UploadResourceContentStreamQuery,
};
use crate::error::HttpError;
use crate::state::HttpState;
use asset_core::CoreError;
use asset_core::domain::{Checksum, ResourceId, ResourceKind, ResourceStatus, StorageKey};
use asset_core::port::BlobByteStream;
use asset_core::port::ListResources;
use asset_core::service::{
    CreateResource, ExecuteResourceAction, UpdateResource, UploadResourceContent,
    UploadResourceContentStream,
};
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
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const MAX_UPLOAD_BYTES: usize = 4 * 1024 * 1024 * 1024;
const DEFAULT_PAGE: u32 = 1;
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;
const DEFAULT_CONTENT_TYPE: &str = "application/octet-stream";

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

/// 列出当前后端支持的资源类型。
#[utoipa::path(
    get,
    path = "/resource-kinds",
    tag = "resources",
    responses(
        (status = 200, description = "资源类型列表", body = ResourceKindsResponse)
    )
)]
pub(crate) async fn list_resource_kinds(
    State(state): State<HttpState>,
) -> Json<ResourceKindsResponse> {
    Json(ResourceKindsResponse {
        items: state
            .kind_registry()
            .list()
            .iter()
            .map(ResourceKindResponse::from)
            .collect(),
    })
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

    Ok((
        StatusCode::CREATED,
        Json(resource_response(state.service(), &resource)?),
    ))
}

/// 分页列出资源。
#[utoipa::path(
    get,
    path = "/resources",
    tag = "resources",
    params(ListResourcesQuery),
    responses(
        (status = 200, description = "资源列表", body = ResourcePageResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn list_resources(
    State(state): State<HttpState>,
    Query(query): Query<ListResourcesQuery>,
) -> Result<Json<ResourcePageResponse>, HttpError> {
    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = u64::from(page - 1) * u64::from(limit);
    let mut command = ListResources::new(limit, offset)
        .with_include_deleted(query.include_deleted.unwrap_or(false));

    if let Some(kind) = query.kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(tag) = query.tag {
        command = command.with_tag(tag);
    }

    if let Some(q) = query.q {
        command = command.with_q(q);
    }

    let page_result = state.service().list_resources(command).await?;

    Ok(Json(ResourcePageResponse {
        items: page_result
            .items
            .iter()
            .map(|resource| resource_response(state.service(), resource))
            .collect::<Result<Vec<_>, _>>()?,
        total: page_result.total,
        page,
        limit: page_result.limit,
    }))
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
    ensure_upload_size(data.len() as u64)?;

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

    Ok((
        StatusCode::CREATED,
        Json(resource_response(state.service(), &resource)?),
    ))
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
    ensure_content_length(&headers)?;
    let data = limited_body_stream(body);

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

    Ok((
        StatusCode::CREATED,
        Json(resource_response(state.service(), &resource)?),
    ))
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
        Some(resource) => Ok(Json(resource_response(state.service(), &resource)?)),
        None => Err(HttpError::not_found(format!("resource `{id}` not found"))),
    }
}

/// 更新资源。
#[utoipa::path(
    patch,
    path = "/resources/{id}",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    request_body = UpdateResourceRequest,
    responses(
        (status = 200, description = "资源已更新", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn update_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateResourceRequest>, JsonRejection>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let payload = parse_json_payload(payload)?;
    let mut command = UpdateResource::new();

    if let Some(name) = payload.name {
        command = command.with_name(name);
    }

    if let Some(kind) = payload.kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(status) = payload.status {
        command = command.with_status(parse_status(&status)?);
    }

    if let Some(metadata) = payload.metadata {
        command = command.with_metadata(metadata.into_domain()?);
    }

    if let Some(restore) = payload.restore {
        command = command.with_restore(restore);
    }

    match state.service().update_resource(&id, command).await? {
        Some(resource) => Ok(Json(resource_response(state.service(), &resource)?)),
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
    let Some(resource) = state.service().find_resource(&id).await? else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };
    let content_type = resource
        .content()
        .and_then(|content| content.mime_type())
        .unwrap_or(DEFAULT_CONTENT_TYPE)
        .to_string();

    match state.service().get_resource_content(&id).await? {
        Some(content) => Ok(binary_response(content_type, content)),
        None => Err(HttpError::not_found(format!(
            "resource content `{id}` not found"
        ))),
    }
}

/// 预览资源内容。
#[utoipa::path(
    get,
    path = "/resources/{id}/preview",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源预览内容", content_type = "application/octet-stream", body = BinaryContent),
        (status = 400, description = "资源类型不支持预览", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源或内容不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn preview_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_resource_id(&id)?;
    let Some(preview) = state.service().preview_resource(&id).await? else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };

    Ok(binary_response(
        preview.content_type().to_string(),
        preview.content().clone(),
    ))
}

/// 读取资源缩略图。
#[utoipa::path(
    get,
    path = "/resources/{id}/thumbnail",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源缩略图", content_type = "application/octet-stream", body = BinaryContent),
        (status = 400, description = "资源类型不支持缩略图", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源或内容不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn thumbnail_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_resource_id(&id)?;
    let Some(thumbnail) = state.service().thumbnail_resource(&id).await? else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };

    Ok(binary_response(
        thumbnail.content_type().to_string(),
        thumbnail.content().clone(),
    ))
}

/// 在线阅读资源内容。
///
/// 当前 MVP 只支持声明 `read` action 的 kind，并要求内容可转换为文本。
#[utoipa::path(
    get,
    path = "/resources/{id}/read",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源阅读内容", body = ResourceReadResponse),
        (status = 400, description = "资源类型不支持阅读或内容格式不支持", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源或内容不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn read_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Json<ResourceReadResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let Some(resource) = state.service().read_resource(&id).await? else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };
    Ok(Json(ResourceReadResponse::from(&resource)))
}

/// 执行资源插件动作。
#[utoipa::path(
    post,
    path = "/resources/{id}/actions/{action}",
    tag = "resources",
    request_body = ExecuteResourceActionRequest,
    params(
        ("id" = String, Path, description = "资源 ID"),
        ("action" = String, Path, description = "动作 ID")
    ),
    responses(
        (status = 200, description = "动作执行结果", body = ResourceActionOutputResponse),
        (status = 400, description = "资源类型不支持该动作或动作未配置", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "插件或服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn execute_resource_action(
    State(state): State<HttpState>,
    Path((id, action)): Path<(String, String)>,
    payload: Result<Json<ExecuteResourceActionRequest>, JsonRejection>,
) -> Result<Json<ResourceActionOutputResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let payload = payload.map_err(|error| HttpError::bad_request(error.to_string()))?;
    let command = ExecuteResourceAction::new(action).with_input(payload.input.clone());
    let Some(output) = state
        .service()
        .execute_resource_action(&id, command)
        .await?
    else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };

    Ok(Json(ResourceActionOutputResponse::from(&output)))
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
        Some(resource) => Ok(Json(resource_response(state.service(), &resource)?)),
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
        command = command.with_kind(parse_kind(kind)?);
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
        command = command.with_kind(parse_kind(kind)?);
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
        command = command.with_kind(parse_kind(kind)?);
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

fn ensure_content_length(headers: &HeaderMap) -> Result<(), HttpError> {
    let Some(value) = headers.get(header::CONTENT_LENGTH) else {
        return Ok(());
    };
    let value = value
        .to_str()
        .map_err(|error| HttpError::bad_request(format!("invalid content-length: {error}")))?;
    let value = value
        .parse::<u64>()
        .map_err(|error| HttpError::bad_request(format!("invalid content-length: {error}")))?;

    ensure_upload_size(value)
}

fn ensure_upload_size(size: u64) -> Result<(), HttpError> {
    if size > MAX_UPLOAD_BYTES as u64 {
        return Err(HttpError::bad_request(format!(
            "request body too large: max {} bytes",
            MAX_UPLOAD_BYTES
        )));
    }

    Ok(())
}

fn limited_body_stream(body: Body) -> BlobByteStream {
    let bytes_read = Arc::new(AtomicU64::new(0));
    let stream_bytes_read = bytes_read.clone();

    Box::pin(body.into_data_stream().map(move |chunk| {
        let chunk = chunk.map_err(|error| CoreError::storage("http.request_body", error))?;
        let total =
            stream_bytes_read.fetch_add(chunk.len() as u64, Ordering::Relaxed) + chunk.len() as u64;

        if total > MAX_UPLOAD_BYTES as u64 {
            return Err(CoreError::configuration(format!(
                "request body too large: max {} bytes",
                MAX_UPLOAD_BYTES
            )));
        }

        Ok(chunk)
    }))
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

fn parse_kind(value: impl Into<String>) -> Result<ResourceKind, HttpError> {
    ResourceKind::try_new(value.into()).map_err(Into::into)
}

fn binary_response(content_type: String, content: Bytes) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, "inline".to_string()),
        ],
        content,
    )
        .into_response()
}

fn resource_response(
    service: &asset_core::service::ResourceService,
    resource: &asset_core::domain::Resource,
) -> Result<ResourceResponse, CoreError> {
    let actions = service.describe_resource_actions(resource)?;
    Ok(ResourceResponse::new(resource, actions))
}

fn parse_resource_id(value: &str) -> Result<ResourceId, HttpError> {
    ResourceId::from_str(value).map_err(|error| HttpError::bad_request(error.to_string()))
}

fn parse_json_payload<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, HttpError> {
    payload
        .map(|Json(payload)| payload)
        .map_err(|error| HttpError::bad_request(format!("invalid JSON request body: {error}")))
}
