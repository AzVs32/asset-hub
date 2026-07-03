pub mod builtin;

use asset_core::CoreError;
use asset_core::port::{ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest};
use async_trait::async_trait;

use crate::plugin::ExtismResourceActionExecutor;

/// Default action executor used by Asset Hub infrastructure.
#[derive(Debug, Clone)]
pub struct DefaultResourceActionExecutor {
    builtin: builtin::BuiltinResourceActionExecutor,
    extism: ExtismResourceActionExecutor,
}

impl DefaultResourceActionExecutor {
    pub fn new(extism: ExtismResourceActionExecutor) -> Self {
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

        self.extism.execute(request).await
    }
}
