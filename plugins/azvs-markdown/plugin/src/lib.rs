use asset_plugin_api::{
    PluginActionEffect, PluginActionOutput, PluginActionRequest, PluginContentEncoding,
    PluginFrameView, PluginView, ReplaceContentEffect, TextView,
};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
#[cfg(target_arch = "wasm32")]
use extism_pdk::host_fn;
use extism_pdk::{Error, FnResult, plugin_fn};
use serde_json::{Value, json};

const VIEWER_ENTRYPOINT: &str = "index.html";
const SAVE_ACTION: &str = "azvs.markdown.update";

#[cfg(target_arch = "wasm32")]
#[host_fn]
extern "ExtismHost" {
    fn asset_hub_content_read(url: String) -> String;
}

#[plugin_fn]
pub fn render_markdown(input: String) -> FnResult<String> {
    render_markdown_payload(input)
}

#[plugin_fn]
pub fn update_markdown(input: String) -> FnResult<String> {
    update_markdown_payload(input)
}

fn render_markdown_payload(input: String) -> FnResult<String> {
    let request: PluginActionRequest = serde_json::from_str(&input)?;
    let markdown = markdown_content(&request)?
        .trim_start_matches('\u{feff}')
        .to_string();
    frame_response(&request, markdown, "read")
}

fn update_markdown_payload(input: String) -> FnResult<String> {
    let request: PluginActionRequest = serde_json::from_str(&input)?;
    if let Some(markdown) = input_markdown(&request.input) {
        let mut output = PluginActionOutput::new(PluginView::Text(TextView {
            text: "Markdown saved".to_string(),
        }));
        output
            .effects
            .push(PluginActionEffect::ReplaceContent(ReplaceContentEffect {
                encoding: PluginContentEncoding::Base64,
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
        return Ok(serde_json::to_string(&output)?);
    }

    let markdown = markdown_content(&request)?
        .trim_start_matches('\u{feff}')
        .to_string();
    frame_response(&request, markdown, "edit")
}

fn frame_response(request: &PluginActionRequest, markdown: String, mode: &str) -> FnResult<String> {
    let payload = json!({
        "mode": mode,
        "resource": {
            "id": request.resource.id,
            "name": request.resource.name,
        },
        "save_action": SAVE_ACTION,
        "markdown": markdown,
    });
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?);
    let output = PluginActionOutput::new(PluginView::PluginFrame(PluginFrameView {
        title: Some(request.resource.name.clone()),
        url: format!("{VIEWER_ENTRYPOINT}#payload={payload}"),
    }));

    Ok(serde_json::to_string(&output)?)
}

fn input_markdown(input: &Value) -> Option<&str> {
    input
        .as_object()
        .and_then(|input| input.get("markdown"))
        .and_then(Value::as_str)
}

fn markdown_content(input: &PluginActionRequest) -> FnResult<String> {
    let base64 = if let Some(content) = &input.content {
        if content.encoding != PluginContentEncoding::Base64 {
            return Err(Error::msg("unsupported content encoding").into());
        }
        content.data.clone()
    } else {
        let content_ref = input
            .content_ref
            .as_ref()
            .ok_or_else(|| Error::msg("missing Markdown content payload"))?;
        if content_ref.encoding != PluginContentEncoding::Url {
            return Err(Error::msg("unsupported content reference encoding").into());
        }
        read_content_ref_base64(&content_ref.url)?
    };

    let bytes = STANDARD.decode(base64)?;
    Ok(String::from_utf8(bytes)?)
}

#[cfg(target_arch = "wasm32")]
fn read_content_ref_base64(url: &str) -> FnResult<String> {
    unsafe { asset_hub_content_read(url.to_string()) }.map_err(Into::into)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_content_ref_base64(_url: &str) -> FnResult<String> {
    Err(Error::msg("content references are only available in the wasm host").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_markdown_returns_plugin_frame() {
        let output = render_markdown_payload(
            json!({
                "action": "azvs.markdown.render",
                "access": "read_only",
                "input": {},
                "resource": resource_json(),
                "content": {
                    "encoding": "base64",
                    "data": STANDARD.encode("# Title\n\nBody")
                }
            })
            .to_string(),
        )
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["view"], "plugin_frame");
        assert_eq!(output["title"], "demo.md");
        assert!(
            output["url"]
                .as_str()
                .unwrap()
                .starts_with("index.html#payload=")
        );
    }

    #[test]
    fn update_markdown_returns_replace_content_effect() {
        let output = update_markdown_payload(
            json!({
                "action": "azvs.markdown.update",
                "access": "read_write",
                "input": {
                    "markdown": "# Updated"
                },
                "resource": resource_json()
            })
            .to_string(),
        )
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["view"], "text");
        assert_eq!(output["text"], "Markdown saved");
        assert_eq!(output["effects"][0]["type"], "replace_content");
        assert_eq!(output["effects"][0]["encoding"], "base64");
        assert_eq!(
            STANDARD
                .decode(output["effects"][0]["data"].as_str().unwrap())
                .unwrap(),
            b"# Updated"
        );
    }

    fn resource_json() -> Value {
        json!({
            "id": "01900000-0000-7000-8000-000000000000",
            "name": "demo.md",
            "kind": "core:document",
            "status": "active",
            "metadata": {},
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
