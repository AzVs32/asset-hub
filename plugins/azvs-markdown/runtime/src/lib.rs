use asset_plugin_api::{
    JsonView, PluginActionEffect, PluginActionFailure, PluginActionOutput, PluginActionRequest,
    PluginContentReferenceEncoding, PluginDiagnostic, PluginFrameView, PluginInlineContentEncoding,
    PluginReplacementEncoding, PluginView, ReplaceContentEffect, TextView,
};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use extism_pdk::{Error, FnResult, plugin_fn};
use serde_json::{Value, json};

const VIEWER_ENTRYPOINT: &str = "index.html";
const SMALL_TEXT_BYTES: u64 = 512 * 1024;
const CONTENT_CHUNK_BYTES: u64 = 2 * 1024 * 1024;
const MAX_MARKDOWN_BYTES: u64 = 128 * 1024 * 1024;

#[plugin_fn]
pub fn render_markdown(input: String) -> FnResult<String> {
    structured_action_result(render_markdown_payload(input))
}

#[plugin_fn]
pub fn update_markdown(input: String) -> FnResult<String> {
    structured_action_result(update_markdown_payload(input))
}

fn structured_action_result(result: FnResult<String>) -> FnResult<String> {
    match result {
        Ok(output) => Ok(output),
        Err(error) => Ok(serde_json::to_string(&PluginActionFailure::new(
            PluginDiagnostic::error(
                asset_plugin_api::diagnostic::codes::ACTION_FAILED,
                error.0.to_string(),
            ),
        ))?),
    }
}

fn render_markdown_payload(input: String) -> FnResult<String> {
    let request: PluginActionRequest = serde_json::from_str(&input)?;
    if input_operation(&request.input).is_some() {
        return content_operation_response(&request);
    }
    frame_response(&request, "read")
}

fn update_markdown_payload(input: String) -> FnResult<String> {
    let request: PluginActionRequest = serde_json::from_str(&input)?;
    if let Some(markdown) = input_markdown(&request.input) {
        return save_response(&request, markdown);
    }
    if input_operation(&request.input).is_some() {
        return content_operation_response(&request);
    }
    frame_response(&request, "edit")
}

fn frame_response(request: &PluginActionRequest, mode: &str) -> FnResult<String> {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
        "resource_id": request.resource.id,
        "mode": mode,
        "action": request.action,
    }))?);
    let output = PluginActionOutput::new(PluginView::PluginFrame(PluginFrameView {
        title: Some(request.resource.name.clone()),
        url: format!("{VIEWER_ENTRYPOINT}#payload={payload}"),
    }));
    Ok(serde_json::to_string(&output)?)
}

fn content_operation_response(request: &PluginActionRequest) -> FnResult<String> {
    let data = match input_operation(&request.input) {
        Some("load") => load_content(request)?,
        Some("chunk") => load_content_chunk(request)?,
        Some(_) => return Err(Error::msg("unsupported Markdown content operation").into()),
        None => return Err(Error::msg("missing Markdown content operation").into()),
    };
    let output = PluginActionOutput::new(PluginView::Json(JsonView { data }));
    Ok(serde_json::to_string(&output)?)
}

fn load_content(request: &PluginActionRequest) -> FnResult<Value> {
    let byte_length = markdown_content_size(request)?;
    ensure_content_size(byte_length)?;
    if byte_length <= SMALL_TEXT_BYTES {
        let bytes = markdown_content_bytes(request)?;
        let markdown = String::from_utf8(bytes)?
            .trim_start_matches('\u{feff}')
            .to_string();
        return Ok(json!({
            "protocol": 1,
            "transfer": "complete",
            "resource_name": request.resource.name,
            "byte_length": byte_length,
            "markdown": markdown,
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

fn load_content_chunk(request: &PluginActionRequest) -> FnResult<Value> {
    let offset = request
        .input
        .get("offset")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::msg("missing or invalid Markdown chunk offset"))?;
    let byte_length = markdown_content_size(request)?;
    ensure_content_size(byte_length)?;
    if offset >= byte_length {
        return Err(Error::msg("Markdown chunk offset is out of range").into());
    }
    let length = CONTENT_CHUNK_BYTES.min(byte_length - offset);
    let bytes = markdown_content_range(request, offset, length)?;
    if bytes.len() as u64 != length {
        return Err(Error::msg("Markdown chunk length does not match the requested range").into());
    }
    Ok(json!({
        "protocol": 1,
        "offset": offset,
        "byte_length": byte_length,
        "data": STANDARD.encode(bytes),
        "done": offset + length == byte_length,
    }))
}

fn save_response(request: &PluginActionRequest, markdown: &str) -> FnResult<String> {
    let mut output = PluginActionOutput::new(PluginView::Text(TextView {
        text: "Markdown saved".to_string(),
    }));
    output
        .effects
        .push(PluginActionEffect::ReplaceContent(ReplaceContentEffect {
            encoding: PluginReplacementEncoding::Base64,
            data: STANDARD.encode(markdown.as_bytes()),
            mime_type: request
                .resource
                .content
                .as_ref()
                .and_then(|content| content.mime_type.clone())
                .or_else(|| Some("text/markdown".to_string())),
            original_filename: request
                .resource
                .content
                .as_ref()
                .and_then(|content| content.original_filename.clone()),
            checksum: Vec::new(),
        }));
    Ok(serde_json::to_string(&output)?)
}

fn input_operation(input: &Value) -> Option<&str> {
    input.get("operation").and_then(Value::as_str)
}

fn input_markdown(input: &Value) -> Option<&str> {
    input.get("markdown").and_then(Value::as_str)
}

fn markdown_content_size(input: &PluginActionRequest) -> FnResult<u64> {
    if let Some(content) = &input.content {
        if content.encoding != PluginInlineContentEncoding::Base64 {
            return Err(Error::msg("unsupported content encoding").into());
        }
        return Ok(STANDARD.decode(&content.data)?.len() as u64);
    }
    let content_ref = input
        .content_ref
        .as_ref()
        .ok_or_else(|| Error::msg("missing Markdown content payload"))?;
    if content_ref.encoding != PluginContentReferenceEncoding::Handle {
        return Err(Error::msg("unsupported content reference encoding").into());
    }
    if content_ref.abi_version != asset_plugin_api::CONTENT_ABI_VERSION {
        return Err(Error::msg("unsupported content ABI version").into());
    }
    input
        .resource
        .content
        .as_ref()
        .map(|content| content.size)
        .ok_or_else(|| Error::msg("missing Markdown content metadata").into())
}

fn markdown_content_bytes(input: &PluginActionRequest) -> FnResult<Vec<u8>> {
    let size = markdown_content_size(input)?;
    markdown_content_range(input, 0, size)
}

fn markdown_content_range(
    input: &PluginActionRequest,
    offset: u64,
    length: u64,
) -> FnResult<Vec<u8>> {
    if let Some(content) = &input.content {
        let bytes = STANDARD.decode(&content.data)?;
        let start = usize::try_from(offset).map_err(|_| Error::msg("Markdown offset overflow"))?;
        let length = usize::try_from(length).map_err(|_| Error::msg("Markdown length overflow"))?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::msg("Markdown content range is out of bounds"))?;
        return Ok(bytes[start..end].to_vec());
    }
    let content_ref = input
        .content_ref
        .as_ref()
        .ok_or_else(|| Error::msg("missing Markdown content payload"))?;
    if content_ref.abi_version != asset_plugin_api::CONTENT_ABI_VERSION {
        return Err(Error::msg("unsupported content ABI version").into());
    }
    read_content_reference_range(&content_ref.reference, offset, length)
}

fn ensure_content_size(size: u64) -> FnResult<()> {
    if size > MAX_MARKDOWN_BYTES {
        return Err(Error::msg("Markdown exceeds the 128 MiB plugin limit").into());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn read_content_reference_range(reference: &str, offset: u64, length: u64) -> FnResult<Vec<u8>> {
    let range = asset_plugin_api::PluginContentRange::new(offset, length)?;
    asset_plugin_api::content::guest::read_range(
        reference,
        range,
        MAX_MARKDOWN_BYTES,
        CONTENT_CHUNK_BYTES,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn read_content_reference_range(_reference: &str, _offset: u64, _length: u64) -> FnResult<Vec<u8>> {
    Err(Error::msg("content references are only available in the wasm host").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_frame_contains_only_small_routing_payload() {
        let output =
            render_markdown_payload(request_json("azvs.markdown.render", json!({}), None)).unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();
        let payload = decode_frame_payload(&output);

        assert_eq!(payload["resource_id"], resource_json()["id"]);
        assert_eq!(payload["mode"], "read");
        assert_eq!(payload["action"], "azvs.markdown.render");
        assert!(payload.get("markdown").is_none());
        assert!(output["url"].as_str().unwrap().len() < 300);
    }

    #[test]
    fn edit_frame_does_not_require_content() {
        let output =
            update_markdown_payload(request_json("azvs.markdown.update", json!({}), None)).unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();
        let payload = decode_frame_payload(&output);
        assert_eq!(payload["mode"], "edit");
        assert!(payload.get("markdown").is_none());
    }

    #[test]
    fn small_markdown_is_returned_directly() {
        let output = render_markdown_payload(request_json(
            "azvs.markdown.render",
            json!({"operation": "load"}),
            Some(b"\xef\xbb\xbf# Title\n\nBody"),
        ))
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output["view"], "json");
        assert_eq!(output["data"]["transfer"], "complete");
        assert_eq!(output["data"]["markdown"], "# Title\n\nBody");
    }

    #[test]
    fn large_markdown_uses_bounded_chunks() {
        let markdown = vec![b'a'; CONTENT_CHUNK_BYTES as usize + 17];
        let load = render_markdown_payload(request_json(
            "azvs.markdown.render",
            json!({"operation": "load"}),
            Some(&markdown),
        ))
        .unwrap();
        let load: Value = serde_json::from_str(&load).unwrap();
        assert_eq!(load["data"]["transfer"], "chunked");
        assert_eq!(load["data"]["chunk_size"], CONTENT_CHUNK_BYTES);

        let chunk = render_markdown_payload(request_json(
            "azvs.markdown.render",
            json!({"operation": "chunk", "offset": CONTENT_CHUNK_BYTES}),
            Some(&markdown),
        ))
        .unwrap();
        let chunk: Value = serde_json::from_str(&chunk).unwrap();
        assert_eq!(chunk["data"]["offset"], CONTENT_CHUNK_BYTES);
        assert_eq!(chunk["data"]["done"], true);
        assert_eq!(
            STANDARD
                .decode(chunk["data"]["data"].as_str().unwrap())
                .unwrap(),
            vec![b'a'; 17]
        );
    }

    #[test]
    fn update_markdown_returns_replace_content_effect() {
        let output = update_markdown_payload(request_json(
            "azvs.markdown.update",
            json!({"markdown": "# Updated"}),
            None,
        ))
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output["view"], "text");
        assert_eq!(output["effects"][0]["type"], "replace_content");
        assert_eq!(
            STANDARD
                .decode(output["effects"][0]["data"].as_str().unwrap())
                .unwrap(),
            b"# Updated"
        );
    }

    fn decode_frame_payload(output: &Value) -> Value {
        let encoded = output["url"]
            .as_str()
            .unwrap()
            .split_once("#payload=")
            .unwrap()
            .1;
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap()
    }

    fn request_json(action: &str, input: Value, content: Option<&[u8]>) -> String {
        let mut request = json!({
            "action": action,
            "access": if action == "azvs.markdown.update" { "read_write" } else { "read_only" },
            "input": input,
            "resource": resource_json(),
        });
        if let Some(content) = content {
            request["content"] = json!({
                "encoding": "base64",
                "data": STANDARD.encode(content),
            });
        }
        request.to_string()
    }

    fn resource_json() -> Value {
        json!({
            "id": "01900000-0000-7000-8000-000000000000",
            "name": "demo.md",
            "kind": "azvs:markdown",
            "status": "active",
            "metadata": {
                "schema_version": 1,
                "summary": {"description": null, "tags": []}
            },
            "content": {
                "key": "documents/demo.md",
                "size": 4,
                "mime_type": "text/markdown",
                "original_filename": "demo.md",
                "checksum": []
            },
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        })
    }
}
