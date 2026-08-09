use super::*;
use asset_plugin_api::protocol::{
    JsonView, PLUGIN_API_VERSION, PluginActionFailure, PluginDiagnostic, PluginFrameView,
    PluginResourceActionOutput, PluginResourceActionRequest, PluginView,
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use extism_pdk::{Error, FnResult, plugin_fn};
use serde_json::json;

#[plugin_fn]
pub fn render_epub(input: String) -> FnResult<String> {
    structured_action_result(render_epub_payload(input))
}

#[plugin_fn]
pub fn render_epub_thumbnail(input: String) -> FnResult<String> {
    structured_action_result(render_epub_thumbnail_payload(input))
}

pub(super) fn structured_action_result(result: FnResult<String>) -> FnResult<String> {
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

pub(super) fn render_epub_payload(input: String) -> FnResult<String> {
    let request: PluginResourceActionRequest = serde_json::from_str(&input)?;
    let operation = request
        .input
        .get("operation")
        .and_then(|value| value.as_str());
    let output = match operation {
        Some("load") => {
            let book = load_cached_book(&request)?;
            let mut index = book.index.clone();
            if !index.chapters.is_empty() {
                index.initial_chapter = render_chapter(&book, 0).ok();
            }
            PluginResourceActionOutput::new(PluginView::Json(JsonView {
                data: serde_json::to_value(index)?,
            }))
        }
        Some("chapter") => {
            let index = request
                .input
                .get("index")
                .and_then(|value| value.as_u64())
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| Error::msg("missing or invalid chapter index"))?;
            let book = load_cached_book(&request)?;
            let chapter = render_chapter(&book, index)?;
            PluginResourceActionOutput::new(PluginView::Json(JsonView {
                data: serde_json::to_value(chapter)?,
            }))
        }
        Some(_) => return Err(Error::msg("unsupported EPUB operation").into()),
        None => {
            let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
                "plugin_api": PLUGIN_API_VERSION,
                "resource_id": &request.resource.id,
                "resource_name": &request.resource.name,
                "action": &request.action,
            }))?);
            PluginResourceActionOutput::new(PluginView::PluginFrame(PluginFrameView {
                plugin_api: PLUGIN_API_VERSION.to_string(),
                title: Some(request.resource.name.clone()),
                url: format!("index.html#payload={payload}"),
            }))
        }
    };

    Ok(serde_json::to_string(&output)?)
}

pub(super) fn render_epub_thumbnail_payload(input: String) -> FnResult<String> {
    let request: PluginResourceActionRequest = serde_json::from_str(&input)?;
    let key = resource_cache_key(&request);
    let cover = if let Some(cover) = cached_cover(&key) {
        cover
    } else if let Some(book) = cached_book(&key) {
        book.index.cover.clone()
    } else {
        let epub = epub_content_bytes(&request)?;
        let cover = render_epub_cover_bytes(&epub)?;
        store_cover(key, cover.clone());
        cover
    };
    let view = match cover {
        Some(cover) => PluginView::Media(cover_media_view(&request.resource.name, &cover)?),
        None => PluginView::Json(JsonView {
            data: serde_json::Value::Null,
        }),
    };
    Ok(serde_json::to_string(&PluginResourceActionOutput::new(
        view,
    ))?)
}
