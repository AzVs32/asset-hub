use asset_plugin_api::protocol::{
    JsonView, PLUGIN_API_VERSION, PluginActionFailure, PluginContentReferenceEncoding,
    PluginDiagnostic, PluginFrameView, PluginInlineContentEncoding, PluginResourceActionOutput,
    PluginResourceActionRequest, PluginView,
};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use extism_pdk::{Error, FnResult, plugin_fn};
use serde_json::{Value, json};

const VIEWER_ENTRYPOINT: &str = "index.html";
const SMALL_TEXT_BYTES: u64 = 512 * 1024;
const CONTENT_CHUNK_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 128 * 1024 * 1024;

#[plugin_fn]
pub fn read_text(input: String) -> FnResult<String> {
    structured_action_result(read_text_payload(input))
}

#[plugin_fn]
pub fn edit_text(input: String) -> FnResult<String> {
    structured_action_result(edit_text_payload(input))
}

fn structured_action_result(result: FnResult<String>) -> FnResult<String> {
    match result {
        Ok(output) => Ok(output),
        Err(error) => Ok(serde_json::to_string(&PluginActionFailure::new(
            PluginDiagnostic::error(
                asset_plugin_api::protocol::diagnostic::codes::ACTION_FAILED,
                error.0.to_string(),
            ),
        ))?),
    }
}

fn read_text_payload(input: String) -> FnResult<String> {
    let request: PluginResourceActionRequest = serde_json::from_str(&input)?;
    if input_operation(&request.input).is_some() {
        return content_operation_response(&request);
    }
    frame_response(&request, "read")
}

fn edit_text_payload(input: String) -> FnResult<String> {
    let request: PluginResourceActionRequest = serde_json::from_str(&input)?;
    if input_operation(&request.input).is_some() {
        return content_operation_response(&request);
    }
    if request.input != json!({}) {
        return Err(Error::msg("unsupported text edit operation").into());
    }
    frame_response(&request, "edit")
}

fn frame_response(request: &PluginResourceActionRequest, mode: &str) -> FnResult<String> {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
        "plugin_api": PLUGIN_API_VERSION,
        "resource_id": request.resource.id,
        "mode": mode,
        "action": request.action,
        "format": text_format(&request.resource.kind, &request.resource.name),
    }))?);
    let output = PluginResourceActionOutput::new(PluginView::PluginFrame(PluginFrameView {
        plugin_api: PLUGIN_API_VERSION.to_string(),
        title: Some(request.resource.name.clone()),
        url: format!("{VIEWER_ENTRYPOINT}#payload={payload}"),
    }));
    Ok(serde_json::to_string(&output)?)
}

fn content_operation_response(request: &PluginResourceActionRequest) -> FnResult<String> {
    let data = match input_operation(&request.input) {
        Some("load") => load_content(request)?,
        Some("chunk") => load_content_chunk(request)?,
        Some(_) => return Err(Error::msg("unsupported text content operation").into()),
        None => return Err(Error::msg("missing text content operation").into()),
    };
    let output = PluginResourceActionOutput::new(PluginView::Json(JsonView { data }));
    Ok(serde_json::to_string(&output)?)
}

fn load_content(request: &PluginResourceActionRequest) -> FnResult<Value> {
    let byte_length = text_content_size(request)?;
    ensure_content_size(byte_length)?;
    if byte_length <= SMALL_TEXT_BYTES {
        let bytes = text_content_bytes(request)?;
        let text = String::from_utf8(bytes)?
            .trim_start_matches('\u{feff}')
            .to_string();
        return Ok(json!({
            "protocol": 1,
            "transfer": "complete",
            "resource_name": request.resource.name,
            "byte_length": byte_length,
            "text": text,
        }));
    }
    Ok(json!({
        "protocol": 1,
        "transfer": "chunked",
        "resource_name": request.resource.name,
        "byte_length": byte_length,
        "chunk_size": CONTENT_CHUNK_BYTES,
    }))
}

fn load_content_chunk(request: &PluginResourceActionRequest) -> FnResult<Value> {
    let offset = request
        .input
        .get("offset")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::msg("missing or invalid text chunk offset"))?;
    let byte_length = text_content_size(request)?;
    ensure_content_size(byte_length)?;
    if offset >= byte_length {
        return Err(Error::msg("text chunk offset is out of range").into());
    }
    let length = CONTENT_CHUNK_BYTES.min(byte_length - offset);
    let bytes = text_content_range(request, offset, length)?;
    if bytes.len() as u64 != length {
        return Err(Error::msg("text chunk length does not match the requested range").into());
    }
    Ok(json!({
        "protocol": 1,
        "offset": offset,
        "byte_length": byte_length,
        "data": STANDARD.encode(bytes),
        "done": offset + length == byte_length,
    }))
}

fn input_operation(input: &Value) -> Option<&str> {
    input.get("operation").and_then(Value::as_str)
}

fn text_content_size(input: &PluginResourceActionRequest) -> FnResult<u64> {
    if let Some(content) = &input.content {
        if content.encoding != PluginInlineContentEncoding::Base64 {
            return Err(Error::msg("unsupported content encoding").into());
        }
        return Ok(STANDARD.decode(&content.data)?.len() as u64);
    }
    let content_ref = input
        .content_ref
        .as_ref()
        .ok_or_else(|| Error::msg("missing text content payload"))?;
    if content_ref.encoding != PluginContentReferenceEncoding::Handle {
        return Err(Error::msg("unsupported content reference encoding").into());
    }
    input
        .resource
        .content
        .as_ref()
        .map(|content| content.size)
        .ok_or_else(|| Error::msg("missing text content description").into())
}

fn text_content_bytes(input: &PluginResourceActionRequest) -> FnResult<Vec<u8>> {
    let size = text_content_size(input)?;
    text_content_range(input, 0, size)
}

fn text_content_range(
    input: &PluginResourceActionRequest,
    offset: u64,
    length: u64,
) -> FnResult<Vec<u8>> {
    if let Some(content) = &input.content {
        let bytes = STANDARD.decode(&content.data)?;
        let start = usize::try_from(offset).map_err(|_| Error::msg("text offset overflow"))?;
        let length = usize::try_from(length).map_err(|_| Error::msg("text length overflow"))?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::msg("text content range is out of bounds"))?;
        return Ok(bytes[start..end].to_vec());
    }
    let content_ref = input
        .content_ref
        .as_ref()
        .ok_or_else(|| Error::msg("missing text content payload"))?;
    read_content_reference_range(&content_ref.reference, offset, length)
}

fn ensure_content_size(size: u64) -> FnResult<()> {
    if size > MAX_TEXT_BYTES {
        return Err(Error::msg("text content exceeds the 128 MiB plugin limit").into());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn read_content_reference_range(reference: &str, offset: u64, length: u64) -> FnResult<Vec<u8>> {
    let range = asset_plugin_api::abi::content::PluginContentRange::new(offset, length)?;
    asset_plugin_api::abi::content::guest::read_range(
        reference,
        range,
        MAX_TEXT_BYTES,
        CONTENT_CHUNK_BYTES,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn read_content_reference_range(_reference: &str, _offset: u64, _length: u64) -> FnResult<Vec<u8>> {
    Err(Error::msg("content references are only available in the wasm host").into())
}

fn text_format(kind: &str, name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if kind == "resource:markdown"
        || [".md", ".markdown", ".mdown", ".mkd"]
            .iter()
            .any(|extension| name.ends_with(extension))
    {
        "markdown"
    } else {
        "plain"
    }
}

#[cfg(test)]
mod tests;
