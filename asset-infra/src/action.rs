pub mod builtin;

use asset_core::CoreError;
use asset_core::port::{
    DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRequest, ResourceActionExecutor,
    ResourceActionOutput, ResourceActionRequest,
};
use async_trait::async_trait;

use crate::plugin::ExtismActionExecutor;

/// Default action executor used by Asset Hub infrastructure.
#[derive(Debug, Clone)]
pub struct DefaultResourceActionExecutor {
    builtin: builtin::BuiltinResourceActionExecutor,
    extism: ExtismActionExecutor,
}

impl DefaultResourceActionExecutor {
    pub fn new(extism: ExtismActionExecutor) -> Self {
        Self {
            builtin: builtin::BuiltinResourceActionExecutor,
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
        if builtin::is_builtin_handler(request.handler()) {
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
    pub fn new(extism: ExtismActionExecutor) -> Self {
        Self {
            builtin: builtin::BuiltinDirectoryActionExecutor,
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
        if builtin::is_builtin_directory_handler(request.handler()) {
            return self.builtin.execute(request).await;
        }
        DirectoryActionExecutor::execute(&self.extism, request).await
    }
}
