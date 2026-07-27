use asset_core::CoreError;
use asset_core::domain::Resource;
use asset_core::port::{
    DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRequest, ResourceActionExecutor,
    ResourceActionOutput, ResourceActionRequest,
};
use asset_plugin_api::{
    DirectoryAction, DirectoryPluginActionOutput, DownloadView, PluginActionOutput, PluginView,
    ResourceAction,
};
use async_trait::async_trait;

const RESOURCE_DOWNLOAD_HANDLER: &str = "builtin.resource.download";
const DIRECTORY_DOWNLOAD_HANDLER: &str = "builtin.directory.download";

#[derive(Debug, Clone, Copy)]
pub struct BuiltinResourceActionExecutor;

pub fn is_builtin_handler(handler: Option<&str>) -> bool {
    matches!(handler, Some(RESOURCE_DOWNLOAD_HANDLER))
}

pub fn is_builtin_directory_handler(handler: Option<&str>) -> bool {
    matches!(handler, Some(DIRECTORY_DOWNLOAD_HANDLER))
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

#[derive(Debug, Clone, Copy)]
pub struct BuiltinDirectoryActionExecutor;

#[async_trait]
impl DirectoryActionExecutor for BuiltinDirectoryActionExecutor {
    async fn execute(
        &self,
        request: DirectoryActionRequest,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let handler = request
            .handler()
            .ok_or_else(|| CoreError::configuration("built-in action is missing handler"))?;
        if handler != DIRECTORY_DOWNLOAD_HANDLER {
            return Err(CoreError::configuration(format!(
                "unknown built-in directory action handler `{handler}`"
            )));
        }
        directory_download(request)
    }
}

fn directory_download(request: DirectoryActionRequest) -> Result<DirectoryActionOutput, CoreError> {
    let directory = request.directory();
    let filename = if directory.id().is_root() {
        "asset-hub.zip".to_string()
    } else {
        format!("{}.zip", directory.directory().name())
    };
    Ok(DirectoryActionOutput::new(
        directory.id(),
        DirectoryAction::from(request.action().as_str()),
        DirectoryPluginActionOutput::new(PluginView::Download(DownloadView {
            url: format!("/directories/{}/download", directory.id()),
            mime_type: Some("application/zip".to_string()),
            filename: Some(filename),
        })),
    ))
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
