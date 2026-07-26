use super::*;

pub(crate) const MAX_UPLOAD_BYTES: usize = 4 * 1024 * 1024 * 1024;
pub(super) const DEFAULT_CONTENT_TYPE: &str = "application/octet-stream";

/// 流式上传内容并创建资源。
///
/// 请求体必须是原始二进制流。资源名称、目录等元信息从 query 参数读取，MIME 类型
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
    access: Extension<AccessContext>,
    Query(query): Query<UploadResourceContentStreamQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<ResourceResponse>), HttpError> {
    let workspace = state.workspace(&access.0).await?;
    let directory = query
        .directory
        .unwrap_or(DirectoryPath::from_path("uploads")?);
    ensure_content_length(&headers)?;
    let data = limited_body_stream(body);

    let mut command = UploadResourceContentStream::new(query.name, data);
    command = command.with_directory(directory);
    command = apply_common_stream_fields(
        command,
        query.kind,
        query.status,
        query.description,
        query.tags_json,
    )?;

    if let Some(mime_type) = content_type(&headers)? {
        command = command.with_mime_type(mime_type);
    }

    let resource = state
        .secured(&access.0)
        .upload_resource_content_stream(command)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(resource_response(state.service(), &workspace, &resource)?),
    ))
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
    access: Extension<AccessContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_resource_id(&id)?;
    let (response, _) = resource_content_response(&state, &access.0, &headers, &id).await?;
    Ok(response)
}

/// 下载资源内容。
#[utoipa::path(
    get,
    path = "/resources/{id}/download",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源下载流", content_type = "application/octet-stream", body = BinaryContent),
        (status = 206, description = "资源部分下载流", content_type = "application/octet-stream", body = BinaryContent),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源内容不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn download_resource_content(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_resource_id(&id)?;
    let (mut response, filename) =
        resource_content_response(&state, &access.0, &headers, &id).await?;
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        attachment_content_disposition(&filename),
    );
    Ok(response)
}

async fn resource_content_response(
    state: &HttpState,
    access: &AccessContext,
    headers: &HeaderMap,
    id: &ResourceId,
) -> Result<(Response, String), HttpError> {
    let Some(resource) = state.secured(access).find_resource(id).await? else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };
    let content_type = resource
        .content()
        .and_then(|content| content.mime_type())
        .unwrap_or(DEFAULT_CONTENT_TYPE)
        .to_string();
    let Some(content_ref) = resource.content() else {
        return Err(HttpError::not_found(format!(
            "resource content `{id}` not found"
        )));
    };
    let range = requested_byte_range(&headers, content_ref.size());

    let response = match range {
        ByteRangeRequest::Unsatisfiable => range_not_satisfiable_response(content_ref.size()),
        ByteRangeRequest::None => match state
            .secured(access)
            .get_resource_content_stream(id, None)
            .await?
        {
            Some(content) => binary_stream_response(
                content_type,
                Some(content.content_length()),
                content.into_content(),
            ),
            None => {
                return Err(HttpError::not_found(format!(
                    "resource content `{id}` not found"
                )));
            }
        },
        ByteRangeRequest::Range { start, end } => match state
            .secured(access)
            .get_resource_content_stream(id, Some((start, end)))
            .await?
        {
            Some(content) => range_stream_response(
                content_type,
                start,
                end,
                content.content_length(),
                content.into_content(),
            ),
            None => {
                return Err(HttpError::not_found(format!(
                    "resource content `{id}` not found"
                )));
            }
        },
    };

    Ok((response, resource.name().to_owned()))
}

pub(super) fn content_type(headers: &HeaderMap) -> Result<Option<String>, HttpError> {
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

pub(super) fn ensure_content_length(headers: &HeaderMap) -> Result<(), HttpError> {
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

pub(super) fn ensure_upload_size(size: u64) -> Result<(), HttpError> {
    if size > MAX_UPLOAD_BYTES as u64 {
        return Err(HttpError::bad_request(format!(
            "request body too large: max {} bytes",
            MAX_UPLOAD_BYTES
        )));
    }

    Ok(())
}

pub(super) fn limited_body_stream(body: Body) -> BlobByteStream {
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

pub(super) fn binary_stream_response(
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
    headers.insert(
        header::ACCEPT_RANGES,
        "bytes".parse().expect("static header value is valid"),
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

fn attachment_content_disposition(filename: &str) -> header::HeaderValue {
    let fallback = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let fallback = if fallback.is_empty() {
        "download"
    } else {
        fallback.as_str()
    };
    format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{}",
        encode_rfc5987(filename)
    )
    .parse()
    .expect("sanitized content disposition should be a valid header value")
}

fn encode_rfc5987(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

pub(super) fn range_stream_response(
    content_type: String,
    start: u64,
    end: u64,
    total_len: u64,
    content: BlobByteStream,
) -> Response {
    let content_length = end - start + 1;
    let mut response = binary_stream_response(content_type, Some(content_length), content);
    *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    response.headers_mut().insert(
        header::CONTENT_RANGE,
        format!("bytes {start}-{end}/{total_len}")
            .parse()
            .expect("content range should be a valid header value"),
    );
    response
}

pub(super) fn range_not_satisfiable_response(total_len: u64) -> Response {
    let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        "bytes".parse().expect("static header value is valid"),
    );
    response.headers_mut().insert(
        header::CONTENT_RANGE,
        format!("bytes */{total_len}")
            .parse()
            .expect("content range should be a valid header value"),
    );
    response
}

pub(super) enum ByteRangeRequest {
    None,
    Range { start: u64, end: u64 },
    Unsatisfiable,
}

pub(super) fn requested_byte_range(headers: &HeaderMap, content_len: u64) -> ByteRangeRequest {
    let Some(range) = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
    else {
        return ByteRangeRequest::None;
    };
    let Some(spec) = range.trim().strip_prefix("bytes=") else {
        return ByteRangeRequest::Unsatisfiable;
    };
    if spec.contains(',') || content_len == 0 {
        return ByteRangeRequest::Unsatisfiable;
    }
    let Some((start, end)) = spec.split_once('-') else {
        return ByteRangeRequest::Unsatisfiable;
    };
    if start.is_empty() {
        let Ok(suffix_len) = end.parse::<u64>() else {
            return ByteRangeRequest::Unsatisfiable;
        };
        if suffix_len == 0 {
            return ByteRangeRequest::Unsatisfiable;
        }
        let start = content_len.saturating_sub(suffix_len);
        return ByteRangeRequest::Range {
            start,
            end: content_len - 1,
        };
    }

    let Ok(start) = start.parse::<u64>() else {
        return ByteRangeRequest::Unsatisfiable;
    };
    if start >= content_len {
        return ByteRangeRequest::Unsatisfiable;
    }
    let end = if end.is_empty() {
        content_len - 1
    } else {
        let Ok(end) = end.parse::<u64>() else {
            return ByteRangeRequest::Unsatisfiable;
        };
        end.min(content_len - 1)
    };
    if end < start {
        return ByteRangeRequest::Unsatisfiable;
    }
    ByteRangeRequest::Range { start, end }
}

#[cfg(test)]
mod tests {
    use super::attachment_content_disposition;

    #[test]
    fn attachment_filename_has_safe_ascii_and_utf8_forms() {
        let value = attachment_content_disposition("报告 2026\".txt");

        assert_eq!(
            value.to_str().unwrap(),
            "attachment; filename=\"___2026_.txt\"; filename*=UTF-8''%E6%8A%A5%E5%91%8A%202026%22.txt"
        );
    }
}
