use asset_core::CoreError;
use asset_core::domain::{DirectoryAction, Resource, ResourceAction};
use asset_core::port::{
    DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRequest, DirectoryKindRegistry,
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest, ResourceKindRegistry,
};
use asset_plugin_api::protocol::directory::DirectoryPluginActionOutput;
use asset_plugin_api::protocol::{
    DownloadView, MediaView, PluginActionOutput, PluginMediaEncoding, PluginView, TextView,
};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use std::sync::Arc;

use crate::builtin_catalog::{
    BuiltinDirectoryAction, BuiltinDirectoryHandler, BuiltinResourceAction, BuiltinResourceHandler,
};

const RESOURCE_THUMBNAIL_SVG: &str = include_str!("../../assets/thumbnails/resource.svg");
const DIRECTORY_THUMBNAIL_SVG: &str = include_str!("../../assets/thumbnails/directory.svg");

#[derive(Debug, Clone)]
pub struct BuiltinResourceActionExecutor {
    bindings: Arc<Vec<BuiltinResourceAction>>,
}

impl BuiltinResourceActionExecutor {
    pub(crate) fn new(
        bindings: &[BuiltinResourceAction],
        kind_registry: &dyn ResourceKindRegistry,
    ) -> Self {
        let bindings = bindings
            .iter()
            .map(|binding| {
                let kinds = kind_registry
                    .definitions()
                    .iter()
                    .filter(|definition| {
                        binding.definition.kinds().iter().any(|kind| {
                            kind_registry
                                .lineage(definition.kind())
                                .iter()
                                .any(|candidate| candidate.as_str() == kind)
                        })
                    })
                    .map(|definition| definition.kind().as_str().to_string())
                    .collect::<Vec<_>>();
                BuiltinResourceAction {
                    definition: binding.definition.clone().with_kinds(kinds),
                    handler: binding.handler,
                }
            })
            .collect();
        Self {
            bindings: Arc::new(bindings),
        }
    }

    pub(crate) fn supports(&self, request: &ResourceActionRequest) -> bool {
        let content = request.resource().content();
        self.bindings.iter().any(|binding| {
            binding.definition.id() == request.action()
                && binding.definition.matches_resource(
                    request.resource().kind().as_str(),
                    content.and_then(|content| content.mime_type()),
                    content.map(|_| request.storage_key().as_str()),
                )
        })
    }
}

#[async_trait]
impl ResourceActionExecutor for BuiltinResourceActionExecutor {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError> {
        let content = request.resource().content();
        let binding = self
            .bindings
            .iter()
            .find(|binding| {
                binding.definition.id() == request.action()
                    && binding.definition.matches_resource(
                        request.resource().kind().as_str(),
                        content.and_then(|content| content.mime_type()),
                        content.map(|_| request.storage_key().as_str()),
                    )
            })
            .ok_or_else(|| {
                CoreError::configuration(format!(
                    "no built-in binding for resource kind `{}` action `{}`",
                    request.resource().kind(),
                    request.action()
                ))
            })?;
        match binding.handler {
            BuiltinResourceHandler::Download => {
                download(request.resource().clone(), request.action().clone())
            }
            BuiltinResourceHandler::GenericThumbnail => {
                resource_thumbnail(request.resource().clone(), request.action().clone())
            }
            BuiltinResourceHandler::ImageThumbnail => {
                image_thumbnail(request.resource().clone(), request.action().clone())
            }
            BuiltinResourceHandler::TextRead => text_read(request),
            BuiltinResourceHandler::TextEdit => text_edit(request),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuiltinDirectoryActionExecutor {
    bindings: Arc<Vec<BuiltinDirectoryAction>>,
}

impl BuiltinDirectoryActionExecutor {
    pub(crate) fn new(
        bindings: &[BuiltinDirectoryAction],
        kind_registry: &dyn DirectoryKindRegistry,
    ) -> Self {
        let bindings = bindings
            .iter()
            .map(|binding| {
                let kinds = kind_registry
                    .definitions()
                    .iter()
                    .filter(|definition| {
                        binding.definition.kinds().iter().any(|kind| {
                            kind_registry
                                .lineage(definition.kind())
                                .iter()
                                .any(|candidate| candidate.as_str() == kind)
                        })
                    })
                    .map(|definition| definition.kind().as_str().to_string())
                    .collect::<Vec<_>>();
                BuiltinDirectoryAction {
                    definition: binding.definition.clone().with_kinds(kinds),
                    handler: binding.handler,
                }
            })
            .collect();
        Self {
            bindings: Arc::new(bindings),
        }
    }

    pub(crate) fn supports(&self, request: &DirectoryActionRequest) -> bool {
        self.bindings.iter().any(|binding| {
            binding.definition.id() == request.action()
                && binding
                    .definition
                    .matches_directory(request.directory().directory().kind().as_str())
        })
    }
}

#[async_trait]
impl DirectoryActionExecutor for BuiltinDirectoryActionExecutor {
    async fn execute(
        &self,
        request: DirectoryActionRequest,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let binding = self
            .bindings
            .iter()
            .find(|binding| {
                binding.definition.id() == request.action()
                    && binding
                        .definition
                        .matches_directory(request.directory().directory().kind().as_str())
            })
            .ok_or_else(|| {
                CoreError::configuration(format!(
                    "no built-in binding for directory kind `{}` action `{}`",
                    request.directory().directory().kind(),
                    request.action()
                ))
            })?;
        match binding.handler {
            BuiltinDirectoryHandler::Download => directory_download(request),
            BuiltinDirectoryHandler::GenericThumbnail => directory_thumbnail(request),
        }
    }
}

fn directory_thumbnail(
    request: DirectoryActionRequest,
) -> Result<DirectoryActionOutput, CoreError> {
    let directory = request.directory();
    let view = embedded_svg_thumbnail(
        Some(if directory.id().is_root() {
            "Asset Hub"
        } else {
            directory.directory().name()
        }),
        DIRECTORY_THUMBNAIL_SVG,
    );
    Ok(DirectoryActionOutput::new(
        directory.id(),
        DirectoryAction::from(request.action().as_str()),
        DirectoryPluginActionOutput::new(view),
    ))
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

fn resource_thumbnail(
    resource: Resource,
    action: ResourceAction,
) -> Result<ResourceActionOutput, CoreError> {
    let view = embedded_svg_thumbnail(Some(resource.name()), RESOURCE_THUMBNAIL_SVG);
    Ok(ResourceActionOutput::new(
        resource.id(),
        action,
        PluginActionOutput::new(view),
    ))
}

fn image_thumbnail(
    resource: Resource,
    action: ResourceAction,
) -> Result<ResourceActionOutput, CoreError> {
    let view = match resource
        .content()
        .and_then(|content| content.mime_type())
        .filter(|mime_type| mime_type.starts_with("image/"))
    {
        Some(mime_type) => PluginView::Media(MediaView {
            mime_type: mime_type.to_string(),
            title: Some(resource.name().to_string()),
            encoding: PluginMediaEncoding::Url,
            data: format!("/resources/{}/content", resource.id()),
        }),
        None => embedded_svg_thumbnail(Some(resource.name()), RESOURCE_THUMBNAIL_SVG),
    };
    Ok(ResourceActionOutput::new(
        resource.id(),
        action,
        PluginActionOutput::new(view),
    ))
}

fn text_read(request: ResourceActionRequest) -> Result<ResourceActionOutput, CoreError> {
    Ok(text_output(
        request.resource().clone(),
        request.action().clone(),
        resource_text(&request)?,
    ))
}

fn text_edit(request: ResourceActionRequest) -> Result<ResourceActionOutput, CoreError> {
    Ok(text_output(
        request.resource().clone(),
        request.action().clone(),
        resource_text(&request)?,
    ))
}

fn resource_text(request: &ResourceActionRequest) -> Result<String, CoreError> {
    let content = request.content().ok_or_else(|| {
        CoreError::configuration("core text actions require inline resource content")
    })?;
    std::str::from_utf8(content)
        .map(|text| text.trim_start_matches('\u{feff}').to_string())
        .map_err(|_| CoreError::configuration("core text actions require valid UTF-8 content"))
}

fn text_output(resource: Resource, action: ResourceAction, text: String) -> ResourceActionOutput {
    ResourceActionOutput::new(
        resource.id(),
        action,
        PluginActionOutput::new(PluginView::Text(TextView { text })),
    )
}

fn embedded_svg_thumbnail(title: Option<&str>, svg: &str) -> PluginView {
    PluginView::Media(MediaView {
        mime_type: "image/svg+xml".to_string(),
        title: title.map(str::to_string),
        encoding: PluginMediaEncoding::Base64,
        data: BASE64_STANDARD.encode(svg.as_bytes()),
    })
}
