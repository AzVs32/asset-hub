use super::*;
use std::io::{Seek, Write};
use tokio_util::io::ReaderStream;
use zip::write::SimpleFileOptions;

pub(super) const DEFAULT_CONTENT_TYPE: &str = "application/octet-stream";
const CONTENT_SHA256: header::HeaderName = header::HeaderName::from_static("content-sha256");

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

/// 流式替换资源文本内容。
#[utoipa::path(
    put,
    path = "/resources/{id}/content",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID"),
        ("If-Match" = String, Header, description = "带双引号的资源 revision"),
        ("Content-SHA256" = String, Header, description = "64 位小写十六进制 SHA-256"),
        ("Content-Length" = u64, Header, description = "原始内容字节数")
    ),
    request_body(
        content = inline(BinaryContent),
        content_type = "application/octet-stream",
        description = "替换后的原始 UTF-8 文本字节"
    ),
    responses(
        (status = 200, description = "资源内容已替换", body = ResourceResponse),
        (status = 400, description = "请求头或资源状态无效", body = crate::dto::ErrorResponse),
        (status = 403, description = "当前工作区没有写权限", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 409, description = "资源 revision、大小或摘要冲突", body = crate::dto::ErrorResponse),
        (status = 413, description = "文本超过 Host 编辑上限", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn replace_resource_content(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let expected_size = required_header(&headers, header::CONTENT_LENGTH, "Content-Length")?
        .parse::<u64>()
        .map_err(|_| HttpError::bad_request("Content-Length must be an unsigned integer"))?;
    let expected_checksum =
        Checksum::sha256(required_header(&headers, CONTENT_SHA256, "Content-SHA256")?)?;
    let expected_revision = parse_if_match(&headers)?;
    let mut command = asset_core::service::ReplaceResourceContent::new(
        expected_size,
        expected_checksum,
        expected_revision,
    );
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        command = command.with_mime_type(
            content_type
                .to_str()
                .map_err(|_| HttpError::bad_request("Content-Type must be valid ASCII"))?,
        );
    }
    let workspace = state.workspace(&access.0).await?;
    let Some(resource) = state
        .secured(&access.0)
        .replace_resource_content(&id, command, body_stream(body))
        .await?
    else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };
    Ok(Json(
        resource_snapshot_response(state.service(), &workspace, &resource).await?,
    ))
}

fn body_stream(body: Body) -> BlobByteStream {
    Box::pin(body.into_data_stream().map(|chunk| {
        chunk.map_err(|error| CoreError::storage("http.resource_content_body", error))
    }))
}

fn required_header<'a>(
    headers: &'a HeaderMap,
    name: header::HeaderName,
    label: &str,
) -> Result<&'a str, HttpError> {
    headers
        .get(name)
        .ok_or_else(|| HttpError::bad_request(format!("missing {label} header")))?
        .to_str()
        .map_err(|_| HttpError::bad_request(format!("{label} must be valid ASCII")))
}

fn parse_if_match(headers: &HeaderMap) -> Result<u64, HttpError> {
    let value = required_header(headers, header::IF_MATCH, "If-Match")?;
    let revision = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| HttpError::bad_request("If-Match must contain a quoted revision"))?;
    revision
        .parse()
        .map_err(|_| HttpError::bad_request("If-Match revision must be an unsigned integer"))
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

/// Download a directory tree as a ZIP archive.
#[utoipa::path(
    get,
    path = "/directories/{id}/download",
    tag = "directories",
    params(("id" = String, Path, description = "Directory ID")),
    responses(
        (status = 200, description = "Directory ZIP archive", content_type = "application/zip", body = BinaryContent),
        (status = 400, description = "Invalid directory ID", body = crate::dto::ErrorResponse),
        (status = 403, description = "Directory is outside the current workspace", body = crate::dto::ErrorResponse),
        (status = 404, description = "Directory or resource content not found", body = crate::dto::ErrorResponse),
        (status = 500, description = "Archive generation failed", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn download_directory(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let id = parse_directory_id(&id)?;
    let manifest = state
        .secured(&access.0)
        .directory_archive_manifest(&id)
        .await?;
    let filename = manifest.filename().to_string();
    let temporary = tempfile::NamedTempFile::new()
        .map_err(|error| CoreError::storage("directory.archive.create", error))?;
    let (file, temporary_path) = temporary.into_parts();
    let mut archive = zip::ZipWriter::new(file);
    let directory_options = SimpleFileOptions::default();

    for directory in manifest.directories() {
        archive
            .add_directory(directory, directory_options)
            .map_err(|error| CoreError::storage("directory.archive.add_directory", error))?;
    }
    for entry in manifest.resources() {
        let file_options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(entry.content_length() > zip::ZIP64_BYTES_THR);
        archive
            .start_file(entry.path(), file_options)
            .map_err(|error| CoreError::storage("directory.archive.start_file", error))?;
        let Some(content) = state
            .secured(&access.0)
            .get_resource_content_stream(&entry.resource_id(), None)
            .await?
        else {
            return Err(HttpError::not_found(format!(
                "resource content `{}` not found",
                entry.resource_id()
            )));
        };
        let mut content = content.into_content();
        while let Some(chunk) = content.next().await {
            archive
                .write_all(&chunk?)
                .map_err(|error| CoreError::storage("directory.archive.write", error))?;
        }
    }

    let mut file = archive
        .finish()
        .map_err(|error| CoreError::storage("directory.archive.finish", error))?;
    file.seek(std::io::SeekFrom::Start(0))
        .map_err(|error| CoreError::storage("directory.archive.rewind", error))?;
    let content_length = file
        .metadata()
        .map_err(|error| CoreError::storage("directory.archive.metadata", error))?
        .len();
    let reader = ReaderStream::new(tokio::fs::File::from_std(file));
    let content = futures_util::stream::try_unfold(
        (reader, temporary_path),
        |(mut reader, temporary_path)| async move {
            match reader.next().await {
                Some(Ok(chunk)) => Ok(Some((chunk, (reader, temporary_path)))),
                Some(Err(error)) => Err(CoreError::storage("directory.archive.read", error)),
                None => Ok(None),
            }
        },
    );
    let mut response = Body::from_stream(content).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        content_length
            .to_string()
            .parse()
            .expect("archive length is a valid header value"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        attachment_content_disposition(&filename),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
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
        .resource()
        .content()
        .and_then(|content| content.mime_type())
        .unwrap_or(DEFAULT_CONTENT_TYPE)
        .to_string();
    let Some(content_ref) = resource.resource().content() else {
        return Err(HttpError::not_found(format!(
            "resource content `{id}` not found"
        )));
    };
    let range = requested_byte_range(headers, content_ref.size());

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

    Ok((response, resource.resource().name().to_owned()))
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
