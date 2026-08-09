//! 目录动作注册与执行端口。
//!
//! 这些端口隔离 Core 与内置动作、插件目录及 Wasm 运行时等基础设施实现。

use crate::{
    CoreError,
    domain::{
        ActionAccess, DirectoryActionDefinition, DirectoryActionId, DirectoryActionRequirements,
        DirectoryId, DirectoryKind,
    },
    port::LocatedDirectory,
};
use asset_plugin_api::protocol::directory::PluginDirectoryActionOutput;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct DirectoryActionRequest {
    directory: LocatedDirectory,
    action: DirectoryActionId,
    access: ActionAccess,
    requirements: DirectoryActionRequirements,
    input: Value,
}

impl DirectoryActionRequest {
    pub fn new(
        directory: LocatedDirectory,
        action: DirectoryActionId,
        access: ActionAccess,
        requirements: DirectoryActionRequirements,
        input: Value,
    ) -> Self {
        Self {
            directory,
            action,
            access,
            requirements,
            input,
        }
    }
    pub fn directory(&self) -> &LocatedDirectory {
        &self.directory
    }
    pub fn action(&self) -> &DirectoryActionId {
        &self.action
    }
    pub fn access(&self) -> ActionAccess {
        self.access
    }
    pub fn requirements(&self) -> &DirectoryActionRequirements {
        &self.requirements
    }
    pub fn input(&self) -> &Value {
        &self.input
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryActionOutput {
    directory_id: DirectoryId,
    action: DirectoryActionId,
    output: PluginDirectoryActionOutput,
}

impl DirectoryActionOutput {
    pub fn new(
        directory_id: DirectoryId,
        action: DirectoryActionId,
        output: PluginDirectoryActionOutput,
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
    pub fn action(&self) -> &DirectoryActionId {
        &self.action
    }
    pub fn output(&self) -> &PluginDirectoryActionOutput {
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

    /// 按具体类型到祖先类型的顺序返回候选动作；同 ID 的更具体定义优先。
    fn action_candidates_for_kinds(
        &self,
        kinds: &[DirectoryKind],
    ) -> Vec<DirectoryActionDefinition> {
        let mut selected = Vec::new();
        for kind in kinds {
            for action in self.actions().iter().filter(|action| {
                action
                    .kinds()
                    .iter()
                    .any(|expected| expected == kind.as_str())
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

    /// 为每个单例能力保留最接近当前 kind 的 provider。
    fn resolve_capability_providers(
        &self,
        actions: Vec<DirectoryActionDefinition>,
    ) -> Vec<DirectoryActionDefinition> {
        let mut provided = HashSet::new();
        actions
            .into_iter()
            .filter(|action| {
                action
                    .provides()
                    .is_none_or(|capability| provided.insert(capability.clone()))
            })
            .collect()
    }

    fn actions_for_kinds(&self, kinds: &[DirectoryKind]) -> Vec<DirectoryActionDefinition> {
        self.resolve_capability_providers(self.action_candidates_for_kinds(kinds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Registry(Vec<DirectoryActionDefinition>);

    impl DirectoryActionRegistry for Registry {
        fn actions(&self) -> &[DirectoryActionDefinition] {
            &self.0
        }
    }

    #[test]
    fn a_specific_provider_replaces_the_selected_generic_action() {
        let registry = Registry(vec![
            DirectoryActionDefinition::new_static("core.directory.thumbnail", "Thumbnail")
                .with_static_provides(Some("thumbnail"))
                .with_kinds(["core:directory"]),
            DirectoryActionDefinition::new_static(
                "example.collection.thumbnail",
                "Collection Thumbnail",
            )
            .with_static_provides(Some("thumbnail"))
            .with_kinds(["example:collection"]),
        ]);
        let lineage = vec![
            DirectoryKind::try_new("example:collection").unwrap(),
            DirectoryKind::try_new("core:directory").unwrap(),
        ];

        let actions = registry.actions_for_kinds(&lineage);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id().as_str(), "example.collection.thumbnail");
    }
}
