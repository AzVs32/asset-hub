use asset_plugin_sdk::{
    Error, Frame, Media, ResourceContext, ResourceResponse, Result, Value, encode_base64,
    encode_base64_url, export_resource_action, json, serde_json,
};

const VIEWER_ENTRYPOINT: &str = "index.html";
const MARKDOWN_THUMBNAIL_SVG: &str = include_str!("../assets/markdown-thumbnail.svg");
const MERMAID_THUMBNAIL_SVG: &str = include_str!("../assets/mermaid-thumbnail.svg");
const SMALL_TEXT_BYTES: u64 = 512 * 1024;
const CONTENT_CHUNK_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 128 * 1024 * 1024;

export_resource_action!(render_thumbnail => handle_thumbnail);
export_resource_action!(read_text => handle_read_text);
export_resource_action!(edit_text => handle_edit_text);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameMode {
    Read,
    Edit,
}

impl FrameMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextFormat {
    Markdown,
    Mermaid,
    Plain,
}

impl TextFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Mermaid => "mermaid",
            Self::Plain => "plain",
        }
    }
}

fn handle_thumbnail(context: ResourceContext) -> Result<ResourceResponse> {
    let resource = context.resource();
    let svg = match detect_text_format(resource.kind(), resource.name()) {
        TextFormat::Markdown => MARKDOWN_THUMBNAIL_SVG,
        TextFormat::Mermaid => MERMAID_THUMBNAIL_SVG,
        TextFormat::Plain => {
            return Err(Error::msg("thumbnail is available only for Markdown and Mermaid").into());
        }
    };
    Ok(ResourceResponse::media(
        Media::base64("image/svg+xml", svg).title(resource.name()),
    ))
}

fn handle_read_text(context: ResourceContext) -> Result<ResourceResponse> {
    if requested_operation(context.input()).is_some() {
        return handle_content_operation(&context);
    }
    text_frame_response(&context, FrameMode::Read)
}

fn handle_edit_text(context: ResourceContext) -> Result<ResourceResponse> {
    if requested_operation(context.input()).is_some() {
        return handle_content_operation(&context);
    }
    if context.input() != &json!({}) {
        return Err(Error::msg("unsupported text edit operation").into());
    }
    text_frame_response(&context, FrameMode::Edit)
}

fn text_frame_response(context: &ResourceContext, mode: FrameMode) -> Result<ResourceResponse> {
    let resource = context.resource();
    let payload = encode_base64_url(serde_json::to_vec(&json!({
        "plugin_api": asset_plugin_sdk::protocol::PLUGIN_API_VERSION,
        "mode": mode.as_str(),
        "action": context.action(),
        "format": detect_text_format(resource.kind(), resource.name()).as_str(),
    }))?);
    Ok(ResourceResponse::frame(
        Frame::new(format!("{VIEWER_ENTRYPOINT}#payload={payload}")).title(resource.name()),
    ))
}

fn handle_content_operation(context: &ResourceContext) -> Result<ResourceResponse> {
    let data = match requested_operation(context.input()) {
        Some("load") => load_text(context)?,
        Some("chunk") => load_text_chunk(context)?,
        Some(_) => return Err(Error::msg("unsupported text content operation").into()),
        None => return Err(Error::msg("missing text content operation").into()),
    };
    ResourceResponse::json(data)
}

fn load_text(context: &ResourceContext) -> Result<Value> {
    let byte_length = context.content().size()?;
    ensure_text_size(byte_length)?;
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

fn load_text_chunk(context: &ResourceContext) -> Result<Value> {
    let offset = context
        .input()
        .get("offset")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::msg("missing or invalid text chunk offset"))?;
    let byte_length = context.content().size()?;
    ensure_text_size(byte_length)?;
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

fn requested_operation(input: &Value) -> Option<&str> {
    input.get("operation").and_then(Value::as_str)
}

fn ensure_text_size(size: u64) -> Result<()> {
    if size > MAX_TEXT_BYTES {
        return Err(Error::msg("text content exceeds the 128 MiB plugin limit").into());
    }
    Ok(())
}

fn detect_text_format(kind: &str, name: &str) -> TextFormat {
    let name = name.to_ascii_lowercase();
    if kind == "resource:mermaid"
        || [".mmd", ".mermaid"]
            .iter()
            .any(|extension| name.ends_with(extension))
    {
        TextFormat::Mermaid
    } else if kind == "resource:markdown"
        || [".md", ".markdown", ".mdown", ".mkd"]
            .iter()
            .any(|extension| name.ends_with(extension))
    {
        TextFormat::Markdown
    } else {
        TextFormat::Plain
    }
}

#[cfg(test)]
mod tests;
