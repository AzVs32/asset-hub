use crate::CoreError;
use crate::domain::Resource;
use crate::port::{ResourceAction, ResourceActionOutput};
use asset_plugin_api::{BinaryUrlView, PluginActionOutput, PluginView};

pub fn execute(
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
