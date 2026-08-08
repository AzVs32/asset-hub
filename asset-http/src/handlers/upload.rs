use super::*;
use axum::http::{HeaderName, HeaderValue};

const UPLOAD_OFFSET: HeaderName = HeaderName::from_static("upload-offset");
const UPLOAD_LENGTH: HeaderName = HeaderName::from_static("upload-length");
const UPLOAD_CHECKSUM: HeaderName = HeaderName::from_static("upload-checksum");

#[utoipa::path(
    post,
    path = "/uploads",
    tag = "uploads",
    request_body = CreateUploadRequest,
    responses(
        (status = 201, description = "上传会话已创建", body = UploadSessionResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 409, description = "目标路径冲突", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn create_upload(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    payload: Result<Json<CreateUploadRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<UploadSessionResponse>), HttpError> {
    let request = parse_json_payload(payload)?;
    let expected_checksum = asset_core::domain::Checksum::sha256(request.expected_sha256)?;
    let mut command = CreateUpload::new(request.name, request.size, expected_checksum)
        .with_directory(request.directory);
    if let Some(kind) = request.kind {
        command = command.with_kind(parse_kind(kind)?);
    }
    if let Some(mime_type) = request.mime_type {
        command = command.with_mime_type(mime_type);
    }
    let session = state.secured(&access.0).create_upload(command).await?;
    Ok((StatusCode::CREATED, Json(session_response(&session))))
}

#[utoipa::path(
    get,
    path = "/uploads/{id}",
    tag = "uploads",
    params(("id" = String, Path, description = "上传会话 ID")),
    responses(
        (status = 200, description = "上传偏移和后台 finalization 状态", body = UploadSessionResponse),
        (status = 404, description = "上传会话不存在", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn upload_status(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
) -> Result<Json<UploadSessionResponse>, HttpError> {
    let id = parse_upload_id(&id)?;
    let session = state.secured(&access.0).upload_status(&id).await?;
    Ok(Json(session_response(&session)))
}

#[utoipa::path(
    patch,
    path = "/uploads/{id}",
    tag = "uploads",
    params(
        ("id" = String, Path, description = "上传会话 ID"),
        ("Upload-Offset" = u64, Header, description = "本分片起始偏移"),
        ("Upload-Checksum" = String, Header, description = "本分片的 64 位小写十六进制 SHA-256")
    ),
    request_body(
        content = inline(BinaryContent),
        content_type = "application/octet-stream",
        description = "从 Upload-Offset 开始的原始文件分片"
    ),
    responses(
        (status = 204, description = "分片已持久化"),
        (status = 409, description = "上传偏移或分片摘要冲突", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn append_upload(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, HeaderMap), HttpError> {
    let id = parse_upload_id(&id)?;
    let offset = parse_offset(&headers)?;
    let expected_chunk_checksum = parse_checksum(&headers)?;
    let session = state
        .secured(&access.0)
        .append_upload(&id, offset, expected_chunk_checksum, body_stream(body))
        .await?;
    Ok((StatusCode::NO_CONTENT, session_headers(&session)?))
}

#[utoipa::path(
    post,
    path = "/uploads/{id}/complete",
    tag = "uploads",
    params(("id" = String, Path, description = "上传会话 ID")),
    responses(
        (status = 202, description = "后台 finalization 已接受", body = UploadSessionResponse),
        (status = 409, description = "上传不完整或目标路径冲突", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn complete_upload(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<UploadSessionResponse>), HttpError> {
    let id = parse_upload_id(&id)?;
    let session = state.secured(&access.0).complete_upload(&id).await?;
    state.dispatch_upload_finalization(id)?;
    Ok((StatusCode::ACCEPTED, Json(session_response(&session))))
}

#[utoipa::path(
    delete,
    path = "/uploads/{id}",
    tag = "uploads",
    params(("id" = String, Path, description = "上传会话 ID")),
    responses(
        (status = 204, description = "上传会话及临时内容已删除"),
        (status = 404, description = "上传会话不存在", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn abort_upload(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpError> {
    let id = parse_upload_id(&id)?;
    state.secured(&access.0).abort_upload(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn body_stream(body: Body) -> BlobByteStream {
    Box::pin(
        body.into_data_stream().map(move |chunk| {
            chunk.map_err(|error| CoreError::storage("http.request_body", error))
        }),
    )
}

fn parse_upload_id(value: &str) -> Result<UploadId, HttpError> {
    UploadId::from_str(value)
        .map_err(|error| HttpError::bad_request(format!("invalid upload id: {error}")))
}

fn parse_offset(headers: &HeaderMap) -> Result<u64, HttpError> {
    headers
        .get(&UPLOAD_OFFSET)
        .ok_or_else(|| HttpError::bad_request("missing Upload-Offset header"))?
        .to_str()
        .map_err(|error| HttpError::bad_request(format!("invalid Upload-Offset: {error}")))?
        .parse()
        .map_err(|error| HttpError::bad_request(format!("invalid Upload-Offset: {error}")))
}

fn parse_checksum(headers: &HeaderMap) -> Result<asset_core::domain::Checksum, HttpError> {
    let value = headers
        .get(&UPLOAD_CHECKSUM)
        .ok_or_else(|| HttpError::bad_request("missing Upload-Checksum header"))?
        .to_str()
        .map_err(|error| HttpError::bad_request(format!("invalid Upload-Checksum: {error}")))?;
    asset_core::domain::Checksum::sha256(value)
        .map_err(|error| HttpError::bad_request(format!("invalid Upload-Checksum: {error}")))
}

fn session_headers(session: &UploadSession) -> Result<HeaderMap, HttpError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        UPLOAD_OFFSET,
        HeaderValue::from_str(&session.offset().to_string())
            .map_err(|error| HttpError::internal(format!("invalid upload offset: {error}")))?,
    );
    headers.insert(
        UPLOAD_LENGTH,
        HeaderValue::from_str(&session.expected_size().to_string())
            .map_err(|error| HttpError::internal(format!("invalid upload length: {error}")))?,
    );
    Ok(headers)
}

fn session_response(session: &UploadSession) -> UploadSessionResponse {
    UploadSessionResponse {
        id: session.id().to_string(),
        offset: session.offset(),
        size: session.expected_size(),
        status: session.status().as_str().to_string(),
        resource_id: (session.status() == asset_core::domain::UploadStatus::Completed)
            .then(|| session.resource_id().to_string()),
        error: session.failure().map(str::to_string),
    }
}
