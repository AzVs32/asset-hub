use asset_plugin_api::protocol::{
    MediaView, PluginActionFailure, PluginDiagnostic, PluginMediaEncoding,
    PluginResourceActionOutput, PluginResourceActionRequest, PluginView,
};
use extism_pdk::{Error, FnResult, plugin_fn};

#[plugin_fn]
pub fn render_thumbnail(input: String) -> FnResult<String> {
    structured_action_result(render_thumbnail_payload(input))
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

fn render_thumbnail_payload(input: String) -> FnResult<String> {
    let request: PluginResourceActionRequest = serde_json::from_str(&input)?;
    if request.resource.content.is_none() {
        return Err(Error::msg("image resource has no content").into());
    }
    let mime_type =
        image_mime_type(&request).ok_or_else(|| Error::msg("resource is not a supported image"))?;
    let output = PluginResourceActionOutput::new(PluginView::Media(MediaView {
        mime_type,
        title: Some(request.resource.name.clone()),
        encoding: PluginMediaEncoding::Url,
        data: format!("/resources/{}/content", request.resource.id),
    }));
    Ok(serde_json::to_string(&output)?)
}

fn image_mime_type(request: &PluginResourceActionRequest) -> Option<String> {
    if let Some(mime_type) = request
        .resource
        .content
        .as_ref()
        .and_then(|content| content.mime_type.as_deref())
        .map(str::trim)
        .filter(|mime_type| mime_type.to_ascii_lowercase().starts_with("image/"))
    {
        return Some(mime_type.to_ascii_lowercase());
    }

    let name = request.resource.name.to_ascii_lowercase();
    let mime_type = if name.ends_with(".png") {
        "image/png"
    } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
        "image/jpeg"
    } else if name.ends_with(".gif") {
        "image/gif"
    } else if name.ends_with(".webp") {
        "image/webp"
    } else if name.ends_with(".svg") {
        "image/svg+xml"
    } else if name.ends_with(".bmp") {
        "image/bmp"
    } else if name.ends_with(".avif") {
        "image/avif"
    } else if name.ends_with(".ico") {
        "image/vnd.microsoft.icon"
    } else if name.ends_with(".tif") || name.ends_with(".tiff") {
        "image/tiff"
    } else {
        return None;
    };
    Some(mime_type.to_string())
}

#[cfg(test)]
mod tests {
    use asset_plugin_api::manifest::PluginManifest;

    #[test]
    fn manifest_is_valid_and_does_not_declare_an_image_kind() {
        let manifest: PluginManifest =
            serde_json::from_str(include_str!("../../manifest.json")).unwrap();

        manifest.validate().unwrap();
        assert!(manifest.capabilities.resource_kinds.is_empty());
        assert_eq!(
            manifest.capabilities.resource_actions[0].id,
            "resource.image.thumbnail"
        );
    }
}
