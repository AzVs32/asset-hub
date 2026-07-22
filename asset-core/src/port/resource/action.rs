//! 资源动作注册与执行端口。
//!
//! 该端口隔离核心服务与插件运行时。插件只能收到核心构造的资源快照、可选对象内容和调用输入；
//! 即使 action 声明为 read_write，也不能直接访问仓储或对象存储。

use crate::CoreError;
use crate::domain::{Resource, ResourceId, ResourceKind};
use asset_plugin_api::{
    PluginActionOutput, ResourceAction, ResourceActionAccess, ResourceActionContentDelivery,
    ResourceActionDefinition,
};
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;

/// 执行资源动作的核心请求。
#[derive(Debug, Clone)]
pub struct ResourceActionRequest {
    resource: Resource,
    action: ResourceAction,
    handler: Option<String>,
    access: ResourceActionAccess,
    content_delivery: ResourceActionContentDelivery,
    input: Value,
    content: Option<Bytes>,
}

impl ResourceActionRequest {
    pub fn new(
        resource: Resource,
        action: ResourceAction,
        handler: Option<impl Into<String>>,
        access: ResourceActionAccess,
        content_delivery: ResourceActionContentDelivery,
        input: Value,
        content: Option<Bytes>,
    ) -> Self {
        Self {
            resource,
            action,
            handler: handler.map(Into::into),
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

    pub fn handler(&self) -> Option<&str> {
        self.handler.as_deref()
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

/// 资源动作的唯一运行时注册表。
pub trait ResourceActionRegistry: Send + Sync {
    /// 返回所有动作定义。多 kind 动作可以按 kind 专门化为同 ID 的上下文定义。
    fn actions(&self) -> &[ResourceActionDefinition];

    /// 按“具体 kind 到祖先 kind”的顺序返回适用动作；同 ID 的更具体定义优先。
    fn actions_for_kinds(&self, kinds: &[ResourceKind]) -> Vec<ResourceActionDefinition> {
        let mut selected = Vec::new();
        for kind in kinds {
            for action in self.actions().iter().filter(|action| {
                action
                    .kinds()
                    .iter()
                    .any(|expected| expected.eq_ignore_ascii_case(kind.as_str()))
            }) {
                if !selected
                    .iter()
                    .any(|existing: &ResourceActionDefinition| existing.id() == action.id())
                {
                    selected.push(action.clone());
                }
            }
        }
        for action in self
            .actions()
            .iter()
            .filter(|action| action.kinds().is_empty())
        {
            if !selected
                .iter()
                .any(|existing: &ResourceActionDefinition| existing.id() == action.id())
            {
                selected.push(action.clone());
            }
        }
        selected
    }
}
