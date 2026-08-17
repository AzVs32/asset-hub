use asset_plugin_sdk::{DirectoryContext, DirectoryResponse, Media, Result};

const THUMBNAIL_SVG: &str = include_str!("../thumbnail.svg");

pub(crate) fn handle(context: DirectoryContext) -> Result<DirectoryResponse> {
    Ok(DirectoryResponse::media(
        Media::base64("image/svg+xml", THUMBNAIL_SVG).title(context.directory().name()),
    ))
}
