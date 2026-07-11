use crate::dto::{
    BinaryContent, CreateDirectoryRequest, CreateResourceRequest, DirectoryListingResponse,
    ExecuteResourceActionRequest, HealthResponse, ListDirectoryQuery, ListResourcesQuery,
    ResourceActionOutputResponse, ResourceDirectoryResponse, ResourceKindResponse,
    ResourceKindsResponse, ResourceMetadataRequest, ResourcePageResponse, ResourceReadResponse,
    ResourceResponse, ScanStorageErrorResponse, ScanStorageRequest, ScanStorageResponse,
    UpdateResourceRequest, UploadResourceContentRequest, UploadResourceContentStreamQuery,
};
use crate::error::HttpError;
use crate::state::HttpState;
use asset_core::CoreError;
use asset_core::domain::{
    AccessContext, Checksum, ResourceId, ResourceKind, ResourceStatus, StorageKey,
};
use asset_core::port::BlobByteStream;
use asset_core::port::ListResources;
use asset_core::service::{
    CreateResource, ExecuteResourceAction, ImportResourceContent, UpdateResource,
    UploadResourceContent, UploadResourceContentStream,
};
use axum::Json;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path as FsPath};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const MAX_UPLOAD_BYTES: usize = 4 * 1024 * 1024 * 1024;
const MAX_BUFFERED_UPLOAD_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_BUFFERED_UPLOAD_REQUEST_BYTES: usize = 48 * 1024 * 1024;
const MAX_SCAN_ENTRIES: usize = 100_000;
const DEFAULT_PAGE: u32 = 1;
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;
const DEFAULT_CONTENT_TYPE: &str = "application/octet-stream";

macro_rules! resource_call {
    ($state:expr, $access:expr, $method:ident($($argument:expr),* $(,)?)) => {{
        if let Some(Extension(context)) = $access.as_ref() {
            $state.secured(context).expect("authenticated router must configure authorization").$method($($argument),*).await
        } else {
            $state.service().$method($($argument),*).await
        }
    }};
}

/// 健康检查。
#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    security(()),
    responses(
        (status = 200, description = "服务正常", body = HealthResponse)
    )
)]
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse::ok())
}

pub(crate) async fn plugin_web_asset(
    State(state): State<HttpState>,
    Path((plugin_id, path)): Path<(String, String)>,
) -> Result<Response, HttpError> {
    let Some(root) = state.plugin_web_root(&plugin_id) else {
        return Err(HttpError::not_found(format!(
            "plugin `{plugin_id}` has no web assets"
        )));
    };
    let Some(relative_path) = clean_plugin_asset_path(&path) else {
        return Err(HttpError::bad_request("invalid plugin asset path"));
    };
    let file_path = root.join(relative_path);
    let bytes = std::fs::read(&file_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            HttpError::not_found(format!("plugin asset `{path}` not found"))
        } else {
            HttpError::from(CoreError::configuration(format!(
                "read plugin asset `{}`: {error}",
                file_path.display()
            )))
        }
    })?;
    let content_type = plugin_asset_content_type(&file_path);

    Ok((
        [(header::CONTENT_TYPE, content_type)],
        axum::body::Body::from(bytes),
    )
        .into_response())
}

fn clean_plugin_asset_path(path: &str) -> Option<std::path::PathBuf> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let mut clean = std::path::PathBuf::new();
    for component in FsPath::new(path).components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

fn plugin_asset_content_type(path: &FsPath) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => DEFAULT_CONTENT_TYPE,
    }
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
            .map(|definition| {
                ResourceKindResponse::from_definition(definition, state.kind_registry())
            })
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
    access: Option<Extension<AccessContext>>,
    payload: Result<Json<CreateResourceRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ResourceResponse>), HttpError> {
    let payload = parse_json_payload(payload)?;
    let command = apply_common_resource_fields(
        CreateResource::new(payload.name),
        payload.kind,
        payload.status,
        payload.directory,
        payload.metadata,
    )?;
    let resource = resource_call!(state, access, create_resource(command))?;

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
    access: Option<Extension<AccessContext>>,
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
    command = command.with_include_descendants(query.include_descendants.unwrap_or(false));

    if let Some(tag) = query.tag {
        command = command.with_tag(tag);
    }

    if let Some(q) = query.q {
        command = command.with_q(q);
    }

    if let Some(directory) = query.directory {
        command = command.with_directory(clean_directory(Some(directory.as_str()), "")?);
    }

    let page_result = resource_call!(state, access, list_resources(command))?;

    Ok(Json(resource_page_response(
        state.service(),
        page_result,
        page,
    )?))
}

/// 列出当前目录的直接子目录和资源。
#[utoipa::path(
    get,
    path = "/directories",
    tag = "resources",
    params(ListDirectoryQuery),
    responses(
        (status = 200, description = "目录列表", body = DirectoryListingResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn list_directory(
    State(state): State<HttpState>,
    access: Option<Extension<AccessContext>>,
    Query(query): Query<ListDirectoryQuery>,
) -> Result<Json<DirectoryListingResponse>, HttpError> {
    let path = clean_directory(query.path.as_deref(), "")?;
    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = u64::from(page - 1) * u64::from(limit);
    let mut resources_query = ListResources::new(limit, offset)
        .with_directory(path.clone())
        .with_include_deleted(query.include_deleted.unwrap_or(false));

    if let Some(kind) = query.kind {
        resources_query = resources_query.with_kind(parse_kind(kind)?);
    }
    resources_query =
        resources_query.with_include_descendants(query.include_descendants.unwrap_or(false));

    if let Some(tag) = query.tag {
        resources_query = resources_query.with_tag(tag);
    }

    if let Some(q) = query.q {
        resources_query = resources_query.with_q(q);
    }

    let folders = resource_call!(state, access, list_directories(&path))?
        .into_iter()
        .map(|directory| ResourceDirectoryResponse {
            path: directory.path().to_owned(),
            parent_path: directory.parent_path().to_owned(),
            name: directory.name().to_owned(),
        })
        .collect();
    let resources = resource_call!(state, access, list_resources(resources_query))?;

    Ok(Json(DirectoryListingResponse {
        path,
        folders,
        resources: resource_page_response(state.service(), resources, page)?,
    }))
}

/// 创建一个空逻辑目录。
#[utoipa::path(
    post,
    path = "/directories",
    tag = "resources",
    request_body = CreateDirectoryRequest,
    responses(
        (status = 201, description = "目录已创建", body = ResourceDirectoryResponse),
        (status = 400, description = "目录名称无效", body = crate::dto::ErrorResponse),
        (status = 403, description = "没有父目录写权限", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn create_directory(
    State(state): State<HttpState>,
    access: Option<Extension<AccessContext>>,
    payload: Result<Json<CreateDirectoryRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ResourceDirectoryResponse>), HttpError> {
    let payload = parse_json_payload(payload)?;
    let parent_path = clean_directory(Some(&payload.parent_path), "")?;
    let directory = resource_call!(state, access, create_directory(parent_path, payload.name))?;
    Ok((
        StatusCode::CREATED,
        Json(ResourceDirectoryResponse {
            path: directory.path().to_owned(),
            parent_path: directory.parent_path().to_owned(),
            name: directory.name().to_owned(),
        }),
    ))
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
    access: Option<Extension<AccessContext>>,
    payload: Option<Json<ScanStorageRequest>>,
) -> Result<Json<ScanStorageResponse>, HttpError> {
    let payload = payload.map(|Json(payload)| payload).unwrap_or_default();
    let path = clean_directory(payload.path.as_deref(), "")?;
    if let Some(Extension(context)) = access.as_ref()
        && !context.is_administrator()
    {
        return Err(HttpError::forbidden(
            "storage scanning is restricted to administrators",
        ));
    }
    let scan_root = if path.is_empty() {
        state.storage_root().clone()
    } else {
        state.storage_root().join(&path)
    };
    ensure_scan_root(state.storage_root(), &scan_root)?;
    let include_sha256 = payload.sha256.unwrap_or(false);
    let files = tokio::task::spawn_blocking({
        let storage_root = state.storage_root().clone();
        move || scan_storage_files(&storage_root, &scan_root, include_sha256)
    })
    .await
    .map_err(|error| CoreError::configuration(format!("scan task failed: {error}")))??;

    let scanned = files.len() as u64;
    let mut imported = Vec::new();
    let mut errors = Vec::new();
    let mut skipped = 0_u64;

    for file in files {
        let mut command = ImportResourceContent::new(
            file.name.clone(),
            StorageKey::new(file.key.clone())?,
            file.size,
        )
        .with_directory(file.directory.clone())
        .with_original_filename(file.name.clone());

        if let Some(mime_type) = file.mime_type {
            command = command.with_mime_type(mime_type);
        }
        if let Some(sha256) = file.sha256 {
            command = command.with_checksum(Checksum::sha256(sha256)?);
        }

        match resource_call!(state, access, import_resource_content(command)) {
            Ok(Some(resource)) => imported.push(resource_response(state.service(), &resource)?),
            Ok(None) => skipped += 1,
            Err(error) => {
                skipped += 1;
                errors.push(ScanStorageErrorResponse {
                    key: file.key,
                    error: error.to_string(),
                });
            }
        }
    }

    Ok(Json(ScanStorageResponse {
        root: state.storage_root().display().to_string(),
        path,
        scanned,
        imported: imported.len() as u64,
        skipped,
        errors,
        resources: imported,
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
    access: Option<Extension<AccessContext>>,
    payload: Result<Json<UploadResourceContentRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ResourceResponse>), HttpError> {
    let payload = parse_json_payload(payload)?;
    let original_filename = payload.original_filename.clone();
    let directory = clean_directory(payload.directory.as_deref(), "uploads")?;
    let storage_key = storage_key_from_upload_parts(
        payload.storage_key,
        Some(directory.clone()),
        original_filename.as_deref().unwrap_or(&payload.name),
    )?;
    let data = BASE64_STANDARD
        .decode(payload.data_base64.as_bytes())
        .map(Bytes::from)
        .map_err(|error| HttpError::bad_request(format!("invalid base64 content: {error}")))?;
    ensure_buffered_upload_size(data.len() as u64)?;

    let mut command = UploadResourceContent::new(payload.name, storage_key, data);
    command = command.with_directory(directory);
    command = apply_common_upload_fields(command, payload.kind, payload.status, payload.metadata)?;

    if let Some(mime_type) = payload.mime_type {
        command = command.with_mime_type(mime_type);
    }

    if let Some(original_filename) = original_filename {
        command = command.with_original_filename(original_filename);
    }

    if let Some(sha256) = payload.sha256 {
        command = command.with_checksum(Checksum::sha256(sha256)?);
    }

    let resource = resource_call!(state, access, upload_resource_content(command))?;

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
    access: Option<Extension<AccessContext>>,
    Query(query): Query<UploadResourceContentStreamQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<ResourceResponse>), HttpError> {
    let directory = clean_directory(query.directory.as_deref(), "uploads")?;
    let storage_key = storage_key_from_upload_parts(
        query.storage_key,
        Some(directory.clone()),
        query.original_filename.as_deref().unwrap_or(&query.name),
    )?;
    ensure_content_length(&headers)?;
    let data = limited_body_stream(body);

    let mut command = UploadResourceContentStream::new(query.name, storage_key, data);
    command = command.with_directory(directory);
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

    let resource = resource_call!(state, access, upload_resource_content_stream(command))?;

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
    access: Option<Extension<AccessContext>>,
    Path(id): Path<String>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;

    match resource_call!(state, access, find_resource(&id))? {
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
    access: Option<Extension<AccessContext>>,
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

    if let Some(directory) = payload.directory {
        command = command.with_directory(clean_directory(Some(&directory), "")?);
    }

    if let Some(metadata) = payload.metadata {
        command = command.with_metadata(metadata.into_domain()?);
    }

    if let Some(restore) = payload.restore {
        command = command.with_restore(restore);
    }

    match resource_call!(state, access, update_resource(&id, command))? {
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
    access: Option<Extension<AccessContext>>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_resource_id(&id)?;
    let Some(resource) = resource_call!(state, access, find_resource(&id))? else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };
    let content_type = resource
        .content()
        .and_then(|content| content.mime_type())
        .unwrap_or(DEFAULT_CONTENT_TYPE)
        .to_string();

    match resource_call!(state, access, get_resource_content(&id))? {
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
    access: Option<Extension<AccessContext>>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_resource_id(&id)?;
    let Some(preview) = resource_call!(state, access, preview_resource_stream(&id))? else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };

    Ok(binary_stream_response(
        preview.content_type().to_string(),
        preview.content_length(),
        preview.into_content(),
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
    access: Option<Extension<AccessContext>>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_resource_id(&id)?;
    let Some(thumbnail) = resource_call!(state, access, thumbnail_resource(&id))? else {
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
    access: Option<Extension<AccessContext>>,
    Path(id): Path<String>,
) -> Result<Json<ResourceReadResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let Some(resource) = resource_call!(state, access, read_resource(&id))? else {
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
    access: Option<Extension<AccessContext>>,
    Path((id, action)): Path<(String, String)>,
    payload: Result<Json<ExecuteResourceActionRequest>, JsonRejection>,
) -> Result<Json<ResourceActionOutputResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let payload = payload.map_err(|error| HttpError::bad_request(error.to_string()))?;
    let command = ExecuteResourceAction::new(action).with_input(payload.input.clone());
    let Some(output) = resource_call!(state, access, execute_resource_action(&id, command))? else {
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
    access: Option<Extension<AccessContext>>,
    Path(id): Path<String>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;

    match resource_call!(state, access, soft_delete_resource(&id))? {
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
    access: Option<Extension<AccessContext>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpError> {
    let id = parse_resource_id(&id)?;

    if resource_call!(state, access, remove_resource(&id))? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(HttpError::not_found(format!("resource `{id}` not found")))
    }
}

/// 物理删除接口已被启动配置禁用。
pub(crate) async fn purge_disabled() -> Result<StatusCode, HttpError> {
    Err(HttpError::forbidden(
        "resource purge endpoint is disabled by ASSET_HTTP_ENABLE_PURGE",
    ))
}

fn apply_common_resource_fields(
    mut command: CreateResource,
    kind: Option<String>,
    status: Option<String>,
    directory: Option<String>,
    metadata: Option<ResourceMetadataRequest>,
) -> Result<CreateResource, HttpError> {
    if let Some(kind) = kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(status) = status {
        command = command.with_status(parse_status(&status)?);
    }

    if let Some(directory) = directory {
        command = command.with_directory(clean_directory(Some(&directory), "")?);
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

fn storage_key_from_upload_parts(
    storage_key: Option<String>,
    directory: Option<String>,
    filename: &str,
) -> Result<StorageKey, HttpError> {
    if let Some(storage_key) = storage_key.filter(|value| !value.trim().is_empty()) {
        return StorageKey::new(storage_key).map_err(Into::into);
    }

    let filename = clean_filename(filename)?;
    let directory = clean_directory(directory.as_deref(), "uploads")?;
    let key = if directory.is_empty() {
        filename
    } else {
        format!("{directory}/{filename}")
    };

    StorageKey::new(key).map_err(Into::into)
}

fn clean_filename(value: &str) -> Result<String, HttpError> {
    let filename = value
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    if filename.is_empty() || filename == "." || filename == ".." {
        return Err(HttpError::bad_request("filename must not be empty"));
    }

    Ok(filename.to_string())
}

fn clean_directory(value: Option<&str>, default: &str) -> Result<String, HttpError> {
    let Some(value) = value else {
        return Ok(default.to_string());
    };
    let value = value.trim().replace('\\', "/");
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.starts_with('/') {
        return Err(HttpError::bad_request("directory must be a relative path"));
    }

    let mut parts = Vec::new();
    for part in value.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(HttpError::bad_request(
                "directory must not contain parent path segments",
            ));
        }
        parts.push(part);
    }

    Ok(parts.join("/"))
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

fn ensure_buffered_upload_size(size: u64) -> Result<(), HttpError> {
    if size > MAX_BUFFERED_UPLOAD_BYTES as u64 {
        return Err(HttpError::bad_request(format!(
            "buffered upload too large: max {} bytes; use the streaming upload endpoint",
            MAX_BUFFERED_UPLOAD_BYTES
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

fn binary_stream_response(
    content_type: String,
    content_length: Option<u64>,
    content: BlobByteStream,
) -> Response {
    let mut response = Body::from_stream(content).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        content_type
            .parse()
            .expect("content type should be a valid header value"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        "inline"
            .parse()
            .expect("content disposition should be a valid header value"),
    );
    if let Some(content_length) = content_length {
        headers.insert(
            header::CONTENT_LENGTH,
            content_length
                .to_string()
                .parse()
                .expect("content length should be a valid header value"),
        );
    }
    response
}

#[derive(Debug)]
struct ScannedStorageFile {
    key: String,
    name: String,
    directory: String,
    size: u64,
    mime_type: Option<String>,
    sha256: Option<String>,
}

fn ensure_scan_root(storage_root: &FsPath, scan_root: &FsPath) -> Result<(), HttpError> {
    let storage_root = storage_root.canonicalize().map_err(|error| {
        CoreError::configuration(format!("storage root is not readable: {error}"))
    })?;
    let scan_root = scan_root
        .canonicalize()
        .map_err(|error| HttpError::bad_request(format!("scan path is not readable: {error}")))?;

    if !scan_root.starts_with(&storage_root) {
        return Err(HttpError::bad_request(
            "scan path must be inside storage root",
        ));
    }

    if !scan_root.is_dir() {
        return Err(HttpError::bad_request("scan path must be a directory"));
    }

    Ok(())
}

fn scan_storage_files(
    storage_root: &FsPath,
    scan_root: &FsPath,
    include_sha256: bool,
) -> Result<Vec<ScannedStorageFile>, CoreError> {
    let storage_root = storage_root.canonicalize().map_err(|error| {
        CoreError::configuration(format!("storage root is not readable: {error}"))
    })?;
    let scan_root = scan_root
        .canonicalize()
        .map_err(|error| CoreError::configuration(format!("scan path is not readable: {error}")))?;
    let mut files = Vec::new();
    let mut visited = 0;
    collect_storage_files(
        &storage_root,
        &scan_root,
        include_sha256,
        &mut visited,
        &mut files,
    )?;
    files.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(files)
}

fn collect_storage_files(
    storage_root: &FsPath,
    current: &FsPath,
    include_sha256: bool,
    visited: &mut usize,
    files: &mut Vec<ScannedStorageFile>,
) -> Result<(), CoreError> {
    let entries =
        std::fs::read_dir(current).map_err(|error| CoreError::storage("scan.read_dir", error))?;

    for entry in entries {
        *visited += 1;
        if *visited > MAX_SCAN_ENTRIES {
            return Err(CoreError::configuration(format!(
                "storage scan exceeds the limit of {MAX_SCAN_ENTRIES} entries"
            )));
        }
        let entry = entry.map_err(|error| CoreError::storage("scan.read_dir_entry", error))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| CoreError::storage("scan.metadata", error))?;

        if metadata.file_type().is_symlink() {
            continue;
        }

        if metadata.is_dir() {
            collect_storage_files(storage_root, &path, include_sha256, visited, files)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        let Some(key) = storage_key_from_file_path(storage_root, &path) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(key.as_str())
            .to_string();
        let directory = key
            .rsplit_once('/')
            .map(|(directory, _)| directory.to_string())
            .unwrap_or_default();
        let sha256 = if include_sha256 {
            Some(sha256_file(&path)?)
        } else {
            None
        };

        files.push(ScannedStorageFile {
            key,
            name,
            directory,
            size: metadata.len(),
            mime_type: content_type_from_path(&path).map(str::to_string),
            sha256,
        });
    }

    Ok(())
}

fn storage_key_from_file_path(storage_root: &FsPath, path: &FsPath) -> Option<String> {
    let relative = path.strip_prefix(storage_root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn sha256_file(path: &FsPath) -> Result<String, CoreError> {
    let mut file = File::open(path).map_err(|error| CoreError::storage("scan.open", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| CoreError::storage("scan.read", error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn content_type_from_path(path: &FsPath) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("txt") => Some("text/plain; charset=utf-8"),
        Some("md") | Some("markdown") => Some("text/markdown; charset=utf-8"),
        Some("json") => Some("application/json"),
        Some("html") | Some("htm") => Some("text/html; charset=utf-8"),
        Some("css") => Some("text/css; charset=utf-8"),
        Some("js") | Some("mjs") => Some("text/javascript"),
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("svg") => Some("image/svg+xml"),
        Some("pdf") => Some("application/pdf"),
        Some("epub") => Some("application/epub+zip"),
        Some("mp3") => Some("audio/mpeg"),
        Some("mp4") => Some("video/mp4"),
        Some("zip") => Some("application/zip"),
        _ => None,
    }
}

fn resource_response(
    service: &asset_core::service::ResourceService,
    resource: &asset_core::domain::Resource,
) -> Result<ResourceResponse, CoreError> {
    let actions = service.describe_resource_actions(resource)?;
    Ok(ResourceResponse::new(resource, actions))
}

fn resource_page_response(
    service: &asset_core::service::ResourceService,
    page_result: asset_core::port::ResourcePage,
    page: u32,
) -> Result<ResourcePageResponse, CoreError> {
    Ok(ResourcePageResponse {
        items: page_result
            .items
            .iter()
            .map(|resource| resource_response(service, resource))
            .collect::<Result<Vec<_>, _>>()?,
        total: page_result.total,
        page,
        limit: page_result.limit,
    })
}

fn parse_resource_id(value: &str) -> Result<ResourceId, HttpError> {
    ResourceId::from_str(value).map_err(|error| HttpError::bad_request(error.to_string()))
}

fn parse_json_payload<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, HttpError> {
    payload
        .map(|Json(payload)| payload)
        .map_err(|error| HttpError::bad_request(format!("invalid JSON request body: {error}")))
}
