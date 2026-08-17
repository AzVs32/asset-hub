use asset_plugin_sdk::{
    Error, Media, ResourceContext, ResourceResponse, ResourceSnapshot, Result,
    export_resource_action,
};

export_resource_action!(render_thumbnail => render_thumbnail_action);

fn render_thumbnail_action(context: ResourceContext) -> Result<ResourceResponse> {
    let resource = context.resource();
    if resource.content_size().is_none() {
        return Err(Error::msg("image resource has no content").into());
    }
    let mime_type =
        image_mime_type(resource).ok_or_else(|| Error::msg("resource is not a supported image"))?;
    Ok(ResourceResponse::media(
        Media::url(mime_type, format!("/resources/{}/content", resource.id()))
            .title(resource.name()),
    ))
}

fn image_mime_type(resource: ResourceSnapshot<'_>) -> Option<String> {
    if let Some(mime_type) = resource
        .mime_type()
        .map(str::trim)
        .filter(|mime_type| mime_type.to_ascii_lowercase().starts_with("image/"))
    {
        return Some(mime_type.to_ascii_lowercase());
    }

    let name = resource.name().to_ascii_lowercase();
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
    use asset_plugin_sdk::manifest::PluginManifest;

    #[test]
    fn manifest_is_valid_and_does_not_declare_an_image_kind() {
        let manifest: PluginManifest =
            asset_plugin_sdk::serde_json::from_str(include_str!("../../manifest.json")).unwrap();

        manifest.validate().unwrap();
        assert!(manifest.capabilities.resource_kinds.is_empty());
        assert_eq!(
            manifest.capabilities.resource_actions[0].id,
            "resource.image.thumbnail"
        );
    }
}
