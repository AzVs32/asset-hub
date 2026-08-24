use asset_rust_sdk::{
    Error, Media, ResourceContext, ResourceResponse, ResourceSnapshot, Result,
    export_resource_action,
};

export_resource_action!(render_thumbnail => render_thumbnail_action);

fn render_thumbnail_action(context: ResourceContext) -> Result<ResourceResponse> {
    let resource = context.resource();
    if resource.content_size().is_none() {
        return Err(Error::msg("image resource has no content"));
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
    use asset_rust_sdk::Value;

    #[test]
    fn manifest_uses_content_matching_without_an_image_kind() {
        let manifest: Value =
            asset_rust_sdk::serde_json::from_str(include_str!("../../manifest.json")).unwrap();

        assert!(
            manifest["capabilities"]["resource_kinds"]
                .as_array()
                .is_some_and(Vec::is_empty)
        );

        let thumbnail = &manifest["capabilities"]["resource_actions"][0];
        assert_eq!(thumbnail["id"], "resource.image.thumbnail");
        assert_eq!(thumbnail["provides"], "thumbnail");
        assert!(thumbnail["applies_to"]["kinds"].is_null());
        assert_eq!(thumbnail["applies_to"]["mime_types"][0], "image/*");
        assert!(
            thumbnail["applies_to"]["extensions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|extension| extension == ".png")
        );
    }
}
