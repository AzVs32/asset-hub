use asset_core::CoreError;
use asset_core::domain::{Resource, ResourceContent};
use asset_core::port::{
    ResourceAction, ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest,
};
use asset_plugin_api::{
    BinaryUrlView, MediaView, PluginActionOutput, PluginContentEncoding, PluginView,
};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;

const MAX_INLINE_BUILTIN_MEDIA_BYTES: u64 = 4 * 1024 * 1024;
const DOWNLOAD_CONTENT_HANDLER: &str = "builtin.content.download";
const MEDIA_VIEW_HANDLER: &str = "builtin.media.view";
const MEDIA_PREVIEW_HANDLER: &str = "builtin.media.preview";
const MEDIA_THUMBNAIL_HANDLER: &str = "builtin.media.thumbnail";

#[derive(Debug, Clone, Copy)]
pub struct BuiltinResourceActionExecutor;

pub fn is_builtin_handler(handler: Option<&str>) -> bool {
    matches!(
        handler,
        Some(
            DOWNLOAD_CONTENT_HANDLER
                | MEDIA_VIEW_HANDLER
                | MEDIA_PREVIEW_HANDLER
                | MEDIA_THUMBNAIL_HANDLER
        )
    )
}

#[async_trait]
impl ResourceActionExecutor for BuiltinResourceActionExecutor {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError> {
        execute(
            request
                .handler()
                .ok_or_else(|| CoreError::configuration("built-in action is missing handler"))?,
            request.resource().clone(),
            request.action().clone(),
            request.content().cloned(),
        )
    }
}

fn execute(
    handler: &str,
    resource: Resource,
    action: ResourceAction,
    content: Option<Bytes>,
) -> Result<ResourceActionOutput, CoreError> {
    match handler {
        DOWNLOAD_CONTENT_HANDLER => download_content(resource, action),
        MEDIA_VIEW_HANDLER | MEDIA_PREVIEW_HANDLER => media_output(resource, action, content),
        MEDIA_THUMBNAIL_HANDLER => thumbnail_output(resource, action, content),
        _ => Err(CoreError::configuration(format!(
            "unknown built-in action handler `{handler}`"
        ))),
    }
}

fn download_content(
    resource: Resource,
    action: ResourceAction,
) -> Result<ResourceActionOutput, CoreError> {
    let Some(content_ref) = resource.content() else {
        return Err(CoreError::not_found(
            "resource content",
            resource.id().to_string(),
        ));
    };

    let view = PluginView::BinaryUrl(BinaryUrlView {
        url: format!("/resources/{}/content", resource.id()),
        mime_type: content_ref.mime_type().map(ToOwned::to_owned),
        filename: content_ref.original_filename().map(ToOwned::to_owned),
    });

    Ok(ResourceActionOutput::new(
        resource.id(),
        action,
        PluginActionOutput::new(view),
    ))
}

fn media_output(
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
    let (encoding, data) = if should_inline_media_for_action(&action, content_ref) {
        let content = content
            .ok_or_else(|| CoreError::not_found("resource content", resource.id().to_string()))?;
        (PluginContentEncoding::Base64, STANDARD.encode(content))
    } else {
        (
            PluginContentEncoding::Url,
            format!("/resources/{}/content", resource.id()),
        )
    };

    let view = PluginView::Media(MediaView {
        mime_type: content_type_for_media(content_ref),
        title: Some(resource.name().to_string()),
        encoding,
        data,
    });

    Ok(ResourceActionOutput::new(
        resource.id(),
        action,
        PluginActionOutput::new(view),
    ))
}

fn thumbnail_output(
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

fn should_inline_media_for_action(action: &ResourceAction, content: &ResourceContent) -> bool {
    action.as_str() != ResourceAction::DOWNLOAD_CONTENT && should_inline_media(content)
}

fn should_inline_media(content: &ResourceContent) -> bool {
    let Some(mime_type) = content.mime_type().map(|value| value.to_ascii_lowercase()) else {
        return content.size() <= MAX_INLINE_BUILTIN_MEDIA_BYTES;
    };

    if mime_type == "application/pdf" || mime_type.starts_with("video/") {
        return false;
    }
    if mime_type.starts_with("image/") {
        return content.size() <= MAX_INLINE_BUILTIN_MEDIA_BYTES;
    }

    mime_type.starts_with("text/")
        || matches!(
            mime_type.as_str(),
            "application/json"
                | "application/xml"
                | "application/javascript"
                | "application/x-javascript"
                | "application/yaml"
                | "application/x-yaml"
        )
}

fn content_type_for_media(content: &ResourceContent) -> String {
    content
        .mime_type()
        .unwrap_or("application/octet-stream")
        .to_string()
}
