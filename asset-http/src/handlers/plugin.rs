use super::*;

pub(crate) async fn plugin_web_asset(
    State(state): State<HttpState>,
    Path((plugin_id, path)): Path<(String, String)>,
) -> Result<Response, HttpError> {
    let Some(relative_path) = clean_plugin_asset_path(&path) else {
        return Err(HttpError::bad_request("invalid plugin asset path"));
    };
    let bytes = state
        .plugin_web_asset(&plugin_id, &relative_path)
        .ok_or_else(|| HttpError::not_found(format!("plugin asset `{path}` not found")))?;
    let content_type = plugin_asset_content_type(&relative_path);

    let mut response = axum::body::Body::from(bytes.to_vec()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        content_type
            .parse()
            .expect("static plugin content type is valid"),
    );
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' data:; font-src 'self' data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'"
            .parse()
            .expect("static plugin CSP is valid"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().expect("static header is valid"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        "*".parse().expect("static header is valid"),
    );
    Ok(response)
}

pub(super) fn clean_plugin_asset_path(path: &str) -> Option<std::path::PathBuf> {
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

pub(super) fn plugin_asset_content_type(path: &FsPath) -> &'static str {
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
