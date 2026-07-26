use asset_core::CoreError;
use asset_core::domain::Resource;
use asset_core::port::{ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest};
use asset_plugin_api::{DownloadView, PluginActionOutput, PluginView, ResourceAction};
use async_trait::async_trait;

const RESOURCE_DOWNLOAD_HANDLER: &str = "builtin.resource.download";

#[derive(Debug, Clone, Copy)]
pub struct BuiltinResourceActionExecutor;

pub fn is_builtin_handler(handler: Option<&str>) -> bool {
    matches!(handler, Some(RESOURCE_DOWNLOAD_HANDLER))
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
        RESOURCE_DOWNLOAD_HANDLER => download(resource, action),
        _ => Err(CoreError::configuration(format!(
            "unknown built-in action handler `{handler}`"
        ))),
    }
}

fn download(resource: Resource, action: ResourceAction) -> Result<ResourceActionOutput, CoreError> {
    let Some(content_ref) = resource.content() else {
        return Err(CoreError::not_found(
            "resource content",
            resource.id().to_string(),
        ));
    };

    let view = PluginView::Download(DownloadView {
        url: format!("/resources/{}/download", resource.id()),
        mime_type: content_ref.mime_type().map(ToOwned::to_owned),
        filename: Some(resource.name().to_owned()),
    });

    Ok(ResourceActionOutput::new(
        resource.id(),
        action,
        PluginActionOutput::new(view),
    ))
}
