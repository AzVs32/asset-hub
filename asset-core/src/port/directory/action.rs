//! 目录动作注册与执行端口。
//!
//! 这些端口隔离 Core 与内置动作、插件目录及 Wasm 运行时等基础设施实现。

use crate::{
    CoreError,
    domain::{DirectoryId, DirectoryKind},
    port::LocatedDirectory,
};
use asset_plugin_api::protocol::directory::DirectoryPluginActionOutput;
use asset_plugin_api::{DirectoryAction, DirectoryActionAccess, DirectoryActionDefinition};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct DirectoryActionRequest {
    directory: LocatedDirectory,
    action: DirectoryAction,
    handler: Option<String>,
    access: DirectoryActionAccess,
    requirements: asset_plugin_api::DirectoryActionRequirements,
    input: Value,
}

impl DirectoryActionRequest {
    pub fn new(
        directory: LocatedDirectory,
        action: DirectoryAction,
        handler: Option<impl Into<String>>,
        access: DirectoryActionAccess,
        requirements: asset_plugin_api::DirectoryActionRequirements,
        input: Value,
    ) -> Self {
        Self {
            directory,
            action,
            handler: handler.map(Into::into),
            access,
            requirements,
            input,
        }
    }
    pub fn directory(&self) -> &LocatedDirectory {
        &self.directory
    }
    pub fn action(&self) -> &DirectoryAction {
        &self.action
    }
    pub fn handler(&self) -> Option<&str> {
        self.handler.as_deref()
    }
    pub fn access(&self) -> DirectoryActionAccess {
        self.access
    }
    pub fn requirements(&self) -> &asset_plugin_api::DirectoryActionRequirements {
        &self.requirements
    }
    pub fn input(&self) -> &Value {
        &self.input
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryActionOutput {
    directory_id: DirectoryId,
    action: DirectoryAction,
    output: DirectoryPluginActionOutput,
}

impl DirectoryActionOutput {
    pub fn new(
        directory_id: DirectoryId,
        action: DirectoryAction,
        output: DirectoryPluginActionOutput,
    ) -> Self {
        Self {
            directory_id,
            action,
            output,
        }
    }
    pub fn directory_id(&self) -> DirectoryId {
        self.directory_id
    }
    pub fn action(&self) -> &DirectoryAction {
        &self.action
    }
    pub fn output(&self) -> &DirectoryPluginActionOutput {
        &self.output
    }
}

/// 目录动作执行端口。
///
/// 执行器只运行声明的处理器并返回结果；聚合更新等 effect 由 Core 校验后落地。
#[async_trait]
pub trait DirectoryActionExecutor: Send + Sync {
    /// 执行一次已经解析并授权的目录动作请求。
    async fn execute(
        &self,
        request: DirectoryActionRequest,
    ) -> Result<DirectoryActionOutput, CoreError>;
}

/// 目录动作定义注册表端口。
///
/// 基础设施适配器负责提供经过启动校验的稳定动作定义集合。
pub trait DirectoryActionRegistry: Send + Sync {
    /// 返回全部目录动作定义。
    fn actions(&self) -> &[DirectoryActionDefinition];

    /// 按具体类型到祖先类型的顺序选择动作；同 ID 的更具体定义优先。
    fn actions_for_kinds(&self, kinds: &[DirectoryKind]) -> Vec<DirectoryActionDefinition> {
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
                    .any(|existing: &DirectoryActionDefinition| existing.id() == action.id())
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
                .any(|existing: &DirectoryActionDefinition| existing.id() == action.id())
            {
                selected.push(action.clone());
            }
        }
        selected
    }
}
