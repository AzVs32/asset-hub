//! 资源动作执行端口。
//!
//! 该端口隔离核心服务与插件运行时。插件只能收到核心构造的资源快照、可选对象内容和调用输入；
//! 即使 action 声明为 read_write，也不能直接访问仓储或对象存储。

use crate::CoreError;
use crate::domain::{Resource, ResourceId};
use crate::port::{ResourceAction, ResourceActionAccess, ResourceActionContentDelivery};
use asset_plugin_api::PluginActionOutput;
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;

/// 执行资源动作的核心请求。
#[derive(Debug, Clone)]
pub struct ResourceActionRequest {
    resource: Resource,
    action: ResourceAction,
    handler: String,
    access: ResourceActionAccess,
    content_delivery: ResourceActionContentDelivery,
    input: Value,
    content: Option<Bytes>,
}

impl ResourceActionRequest {
    pub fn new(
        resource: Resource,
        action: ResourceAction,
        handler: impl Into<String>,
        access: ResourceActionAccess,
        content_delivery: ResourceActionContentDelivery,
        input: Value,
        content: Option<Bytes>,
    ) -> Self {
        Self {
            resource,
            action,
            handler: handler.into(),
            access,
            content_delivery,
            input,
            content,
        }
    }

    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    pub fn action(&self) -> &ResourceAction {
        &self.action
    }

    pub fn handler(&self) -> &str {
        &self.handler
    }

    pub fn access(&self) -> ResourceActionAccess {
        self.access
    }

    pub fn content_delivery(&self) -> ResourceActionContentDelivery {
        self.content_delivery
    }

    pub fn input(&self) -> &Value {
        &self.input
    }

    pub fn content(&self) -> Option<&Bytes> {
        self.content.as_ref()
    }
}

/// 执行动作后的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceActionOutput {
    resource_id: ResourceId,
    action: ResourceAction,
    output: PluginActionOutput,
}

impl ResourceActionOutput {
    pub fn new(
        resource_id: ResourceId,
        action: ResourceAction,
        output: PluginActionOutput,
    ) -> Self {
        Self {
            resource_id,
            action,
            output,
        }
    }

    pub fn resource_id(&self) -> ResourceId {
        self.resource_id
    }

    pub fn action(&self) -> &ResourceAction {
        &self.action
    }

    pub fn output(&self) -> &PluginActionOutput {
        &self.output
    }
}

/// 资源动作执行器。
#[async_trait]
pub trait ResourceActionExecutor: Send + Sync {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError>;
}
