use asset_core::CoreError;
use asset_core::domain::{DirectoryActionId, Resource, ResourceActionId};
use asset_core::port::{
    DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRequest, DirectoryKindRegistry,
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest, ResourceKindRegistry,
};
use asset_plugin_api::protocol::directory::DirectoryActionEffect;
use asset_plugin_api::protocol::directory::PluginDirectoryActionOutput;
use asset_plugin_api::protocol::{
    DownloadView, PluginResourceActionEffect, PluginResourceActionOutput, PluginView,
};
use async_trait::async_trait;
use std::sync::Arc;

use crate::builtin_catalog::{
    BuiltinDirectoryAction, BuiltinDirectoryHandler, BuiltinResourceAction, BuiltinResourceHandler,
};

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
            binding.definition.id().as_str() == request.action().as_str()
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
                binding.definition.id().as_str() == request.action().as_str()
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
            BuiltinResourceHandler::Delete => resource_delete(request),
            BuiltinResourceHandler::Download => {
                download(request.resource().clone(), request.action().clone())
            }
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
            binding.definition.id().as_str() == request.action().as_str()
                && binding
                    .definition
                    .matches_exact_kind(request.directory().directory().kind().as_str())
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
                binding.definition.id().as_str() == request.action().as_str()
                    && binding
                        .definition
                        .matches_exact_kind(request.directory().directory().kind().as_str())
            })
            .ok_or_else(|| {
                CoreError::configuration(format!(
                    "no built-in binding for directory kind `{}` action `{}`",
                    request.directory().directory().kind(),
                    request.action()
                ))
            })?;
        match binding.handler {
            BuiltinDirectoryHandler::Delete => directory_delete(request),
            BuiltinDirectoryHandler::Download => directory_download(request),
        }
    }
}

fn directory_delete(request: DirectoryActionRequest) -> Result<DirectoryActionOutput, CoreError> {
    let mut output = PluginDirectoryActionOutput::without_view();
    output.effects.push(DirectoryActionEffect::Delete);
    Ok(DirectoryActionOutput::new(
        request.directory().id(),
        request.action().clone(),
        output,
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
        DirectoryActionId::new(request.action().as_str()).map_err(CoreError::from)?,
        PluginDirectoryActionOutput::new(PluginView::Download(DownloadView {
            url: format!("/directories/{}/download", directory.id()),
            mime_type: Some("application/zip".to_string()),
            filename: Some(filename),
        })),
    ))
}

fn download(
    resource: Resource,
    action: ResourceActionId,
) -> Result<ResourceActionOutput, CoreError> {
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
        PluginResourceActionOutput::new(view),
    ))
}

fn resource_delete(request: ResourceActionRequest) -> Result<ResourceActionOutput, CoreError> {
    let mut output = PluginResourceActionOutput::without_view();
    output.effects.push(PluginResourceActionEffect::Delete);
    Ok(ResourceActionOutput::new(
        request.resource().id(),
        request.action().clone(),
        output,
    ))
}
