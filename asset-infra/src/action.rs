mod builtin;

use asset_core::CoreError;
use asset_core::port::{
    DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRequest, DirectoryKindRegistry,
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest, ResourceKindRegistry,
};
use async_trait::async_trait;

use crate::plugin::ExtismActionExecutor;
use crate::plugin_manifest::PluginCatalog;

/// Default action executor used by Asset Hub infrastructure.
#[derive(Debug, Clone)]
pub struct DefaultResourceActionExecutor {
    builtin: builtin::BuiltinResourceActionExecutor,
    extism: ExtismActionExecutor,
}

impl DefaultResourceActionExecutor {
    pub fn new(
        catalog: &PluginCatalog,
        kind_registry: &dyn ResourceKindRegistry,
        extism: ExtismActionExecutor,
    ) -> Self {
        Self {
            builtin: builtin::BuiltinResourceActionExecutor::new(
                &catalog.builtin.resource_actions,
                kind_registry,
            ),
            extism,
        }
    }
}

#[async_trait]
impl ResourceActionExecutor for DefaultResourceActionExecutor {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError> {
        if self.builtin.supports(&request) {
            return self.builtin.execute(request).await;
        }

        ResourceActionExecutor::execute(&self.extism, request).await
    }
}

#[derive(Debug, Clone)]
pub struct DefaultDirectoryActionExecutor {
    builtin: builtin::BuiltinDirectoryActionExecutor,
    extism: ExtismActionExecutor,
}

impl DefaultDirectoryActionExecutor {
    pub fn new(
        catalog: &PluginCatalog,
        kind_registry: &dyn DirectoryKindRegistry,
        extism: ExtismActionExecutor,
    ) -> Self {
        Self {
            builtin: builtin::BuiltinDirectoryActionExecutor::new(
                &catalog.builtin.directory_actions,
                kind_registry,
            ),
            extism,
        }
    }
}

#[async_trait]
impl DirectoryActionExecutor for DefaultDirectoryActionExecutor {
    async fn execute(
        &self,
        request: DirectoryActionRequest,
    ) -> Result<DirectoryActionOutput, CoreError> {
        if self.builtin.supports(&request) {
            return self.builtin.execute(request).await;
        }
        DirectoryActionExecutor::execute(&self.extism, request).await
    }
}
