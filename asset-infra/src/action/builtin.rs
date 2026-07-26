use asset_core::CoreError;
use asset_core::domain::Resource;
use asset_core::port::{ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest};
use asset_plugin_api::{BinaryUrlView, PluginActionOutput, PluginView, ResourceAction};
use async_trait::async_trait;

const DOWNLOAD_CONTENT_HANDLER: &str = "builtin.content.download";

#[derive(Debug, Clone, Copy)]
pub struct BuiltinResourceActionExecutor;

pub fn is_builtin_handler(handler: Option<&str>) -> bool {
    matches!(handler, Some(DOWNLOAD_CONTENT_HANDLER))
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
        )
    }
}

fn execute(
    handler: &str,
    resource: Resource,
    action: ResourceAction,
) -> Result<ResourceActionOutput, CoreError> {
    match handler {
        DOWNLOAD_CONTENT_HANDLER => download_content(resource, action),
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
        filename: Some(resource.name().to_owned()),
    });

    Ok(ResourceActionOutput::new(
        resource.id(),
        action,
        PluginActionOutput::new(view),
    ))
}
