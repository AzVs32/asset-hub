pub mod download_content;
pub mod preview;
pub mod thumbnail;
pub mod view_inline;

use crate::CoreError;
use crate::domain::{Resource, ResourceContent};
use crate::port::{ResourceAction, ResourceActionOutput};
use asset_plugin_api::{MediaView, PluginActionOutput, PluginContentEncoding, PluginView};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;

pub fn execute(
    resource: Resource,
    action: ResourceAction,
    content: Option<Bytes>,
) -> Result<ResourceActionOutput, CoreError> {
    match action.as_str() {
        ResourceAction::DOWNLOAD_CONTENT => download_content::execute(resource, action),
        ResourceAction::VIEW_INLINE => view_inline::execute(resource, action, content),
        ResourceAction::PREVIEW => preview::execute(resource, action, content),
        ResourceAction::THUMBNAIL => thumbnail::execute(resource, action, content),
        _ => Err(CoreError::configuration(format!(
            "resource kind `{}` does not support built-in action `{action}` for this content",
            resource.kind()
        ))),
    }
}

pub(crate) fn media_output(
    resource: Resource,
    action: ResourceAction,
    content: Option<Bytes>,
) -> Result<ResourceActionOutput, CoreError> {
    let Some(content_ref) = resource.content() else {
        return Err(CoreError::not_found(
            "resource content",
            resource.id().to_string(),
        ));
    };
    let content = content
        .ok_or_else(|| CoreError::not_found("resource content", resource.id().to_string()))?;

    let view = PluginView::Media(MediaView {
        mime_type: content_type_for_media(content_ref),
        title: Some(resource.name().to_string()),
        encoding: PluginContentEncoding::Base64,
        data: STANDARD.encode(content),
    });

    Ok(ResourceActionOutput::new(
        resource.id(),
        action,
        PluginActionOutput::new(view),
    ))
}

pub fn decode_media_view(action: &str, view: &PluginView) -> Result<(String, Bytes), CoreError> {
    let PluginView::Media(media) = view else {
        return Err(CoreError::configuration(format!(
            "resource action `{action}` must return a media view"
        )));
    };
    if media.encoding != PluginContentEncoding::Base64 {
        return Err(CoreError::configuration(format!(
            "resource action `{action}` returned unsupported media encoding"
        )));
    }
    let content = STANDARD.decode(&media.data).map_err(|error| {
        CoreError::configuration(format!(
            "resource action `{action}` returned invalid media: {error}"
        ))
    })?;

    Ok((media.mime_type.clone(), Bytes::from(content)))
}

pub fn content_type_for_media(content: &ResourceContent) -> String {
    content
        .mime_type()
        .unwrap_or("application/octet-stream")
        .to_string()
}
