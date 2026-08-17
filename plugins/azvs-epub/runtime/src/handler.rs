use super::*;
use asset_plugin_sdk::{Frame, Value, encode_base64_url, export_resource_action};
use serde_json::json;

export_resource_action!(render_epub => render_epub_payload);
export_resource_action!(render_epub_thumbnail => render_epub_thumbnail_payload);

pub(super) fn render_epub_payload(context: ResourceContext) -> Result<ResourceResponse> {
    let operation = context
        .input()
        .get("operation")
        .and_then(|value| value.as_str());
    match operation {
        Some("load") => {
            let book = load_cached_book(&context)?;
            let mut index = book.index.clone();
            if !index.chapters.is_empty() {
                index.initial_chapter = render_chapter(&book, 0).ok();
            }
            ResourceResponse::json(index)
        }
        Some("chapter") => {
            let index = context
                .input()
                .get("index")
                .and_then(|value| value.as_u64())
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| Error::msg("missing or invalid chapter index"))?;
            let book = load_cached_book(&context)?;
            let chapter = render_chapter(&book, index)?;
            ResourceResponse::json(chapter)
        }
        Some(_) => Err(Error::msg("unsupported EPUB operation").into()),
        None => {
            let resource = context.resource();
            let payload = encode_base64_url(serde_json::to_vec(&json!({
                "plugin_api": asset_plugin_sdk::protocol::PLUGIN_API_VERSION,
                "resource_id": resource.id(),
                "resource_name": resource.name(),
                "action": context.action(),
            }))?);
            Ok(ResourceResponse::frame(
                Frame::new(format!("index.html#payload={payload}")).title(resource.name()),
            ))
        }
    }
}

pub(super) fn render_epub_thumbnail_payload(context: ResourceContext) -> Result<ResourceResponse> {
    let key = resource_cache_key(&context);
    let cover = if let Some(cover) = cached_cover(&key) {
        cover
    } else if let Some(book) = cached_book(&key) {
        book.index.cover.clone()
    } else {
        let epub = epub_content_bytes(&context)?;
        let cover = render_epub_cover_bytes(&epub)?;
        store_cover(key, cover.clone());
        cover
    };
    match cover {
        Some(cover) => Ok(ResourceResponse::media(cover_media_view(
            context.resource().name(),
            &cover,
        )?)),
        None => ResourceResponse::json(Value::Null),
    }
}
