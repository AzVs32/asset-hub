mod builtin;

use asset_core::CoreError;
use asset_core::port::{
    DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRequest, DirectoryKindRegistry,
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest, ResourceKindRegistry,
};
use async_trait::async_trait;

use crate::builtin_catalog::{BuiltinDirectoryAction, BuiltinResourceAction};

/// Default action executor used by Asset Hub infrastructure.
#[derive(Debug, Clone)]
pub struct DefaultResourceActionExecutor {
    builtin: builtin::BuiltinResourceActionExecutor,
}

impl DefaultResourceActionExecutor {
    pub fn new(
        bindings: &[BuiltinResourceAction],
        kind_registry: &dyn ResourceKindRegistry,
    ) -> Self {
        Self {
            builtin: builtin::BuiltinResourceActionExecutor::new(bindings, kind_registry),
        }
    }
}

#[async_trait]
impl ResourceActionExecutor for DefaultResourceActionExecutor {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError> {
        self.builtin.execute(request).await
    }
}

#[derive(Debug, Clone)]
pub struct DefaultDirectoryActionExecutor {
    builtin: builtin::BuiltinDirectoryActionExecutor,
}

impl DefaultDirectoryActionExecutor {
    pub fn new(
        bindings: &[BuiltinDirectoryAction],
        kind_registry: &dyn DirectoryKindRegistry,
    ) -> Self {
        Self {
            builtin: builtin::BuiltinDirectoryActionExecutor::new(bindings, kind_registry),
        }
    }
}

#[async_trait]
impl DirectoryActionExecutor for DefaultDirectoryActionExecutor {
    async fn execute(
        &self,
        request: DirectoryActionRequest,
    ) -> Result<DirectoryActionOutput, CoreError> {
        self.builtin.execute(request).await
    }
}
