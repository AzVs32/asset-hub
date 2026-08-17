use asset_plugin_sdk::{
    Error, Frame, ResourceContext, ResourceResponse, Result, Value, encode_base64,
    encode_base64_url, export_resource_action, json, serde_json,
};

const VIEWER_ENTRYPOINT: &str = "index.html";
const SMALL_TEXT_BYTES: u64 = 512 * 1024;
const CONTENT_CHUNK_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 128 * 1024 * 1024;

export_resource_action!(read_text => read_text_payload);
export_resource_action!(edit_text => edit_text_payload);

fn read_text_payload(context: ResourceContext) -> Result<ResourceResponse> {
    if input_operation(context.input()).is_some() {
        return content_operation_response(&context);
    }
    frame_response(&context, "read")
}

fn edit_text_payload(context: ResourceContext) -> Result<ResourceResponse> {
    if input_operation(context.input()).is_some() {
        return content_operation_response(&context);
    }
    if context.input() != &json!({}) {
        return Err(Error::msg("unsupported text edit operation").into());
    }
    frame_response(&context, "edit")
}

fn frame_response(context: &ResourceContext, mode: &str) -> Result<ResourceResponse> {
    let resource = context.resource();
    let payload = encode_base64_url(serde_json::to_vec(&json!({
        "plugin_api": asset_plugin_sdk::protocol::PLUGIN_API_VERSION,
        "resource_id": resource.id(),
        "mode": mode,
        "action": context.action(),
        "format": text_format(resource.kind(), resource.name()),
    }))?);
    Ok(ResourceResponse::frame(
        Frame::new(format!("{VIEWER_ENTRYPOINT}#payload={payload}")).title(resource.name()),
    ))
}

fn content_operation_response(context: &ResourceContext) -> Result<ResourceResponse> {
    let data = match input_operation(context.input()) {
        Some("load") => load_content(context)?,
        Some("chunk") => load_content_chunk(context)?,
        Some(_) => return Err(Error::msg("unsupported text content operation").into()),
        None => return Err(Error::msg("missing text content operation").into()),
    };
    ResourceResponse::json(data)
}

fn load_content(context: &ResourceContext) -> Result<Value> {
    let byte_length = context.content().size()?;
    ensure_content_size(byte_length)?;
    if byte_length <= SMALL_TEXT_BYTES {
        let bytes = context
            .content()
            .read_all(MAX_TEXT_BYTES, CONTENT_CHUNK_BYTES)?;
        let text = String::from_utf8(bytes)?
            .trim_start_matches('\u{feff}')
            .to_string();
        return Ok(json!({
            "protocol": 1,
            "transfer": "complete",
            "resource_name": context.resource().name(),
            "byte_length": byte_length,
            "text": text,
        }));
    }
    Ok(json!({
        "protocol": 1,
        "transfer": "chunked",
        "resource_name": context.resource().name(),
        "byte_length": byte_length,
        "chunk_size": CONTENT_CHUNK_BYTES,
    }))
}

fn load_content_chunk(context: &ResourceContext) -> Result<Value> {
    let offset = context
        .input()
        .get("offset")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::msg("missing or invalid text chunk offset"))?;
    let byte_length = context.content().size()?;
    ensure_content_size(byte_length)?;
    if offset >= byte_length {
        return Err(Error::msg("text chunk offset is out of range").into());
    }
    let length = CONTENT_CHUNK_BYTES.min(byte_length - offset);
    let bytes =
        context
            .content()
            .read_range(offset, length, MAX_TEXT_BYTES, CONTENT_CHUNK_BYTES)?;
    if bytes.len() as u64 != length {
        return Err(Error::msg("text chunk length does not match the requested range").into());
    }
    Ok(json!({
        "protocol": 1,
        "offset": offset,
        "byte_length": byte_length,
        "data": encode_base64(bytes),
        "done": offset + length == byte_length,
    }))
}

fn input_operation(input: &Value) -> Option<&str> {
    input.get("operation").and_then(Value::as_str)
}

fn ensure_content_size(size: u64) -> Result<()> {
    if size > MAX_TEXT_BYTES {
        return Err(Error::msg("text content exceeds the 128 MiB plugin limit").into());
    }
    Ok(())
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
