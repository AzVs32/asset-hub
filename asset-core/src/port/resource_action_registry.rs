//! 资源动作注册表端口。

use crate::domain::ResourceKind;
use asset_plugin_api::ResourceActionDefinition;

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

#[cfg(test)]
mod tests {
    use super::*;

    struct Registry(Vec<ResourceActionDefinition>);

    impl ResourceActionRegistry for Registry {
        fn actions(&self) -> &[ResourceActionDefinition] {
            &self.0
        }
    }

    #[test]
    fn actions_are_selected_from_the_supplied_kind_lineage() {
        let registry = Registry(vec![
            ResourceActionDefinition::new("document.open", "Open").with_kinds(["core:document"]),
            ResourceActionDefinition::new("image.view", "View").with_kinds(["core:image"]),
        ]);
        let lineage = vec![
            ResourceKind::from("code:c"),
            ResourceKind::from("core:document"),
        ];

        let actions = registry.actions_for_kinds(&lineage);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id().as_str(), "document.open");
    }
}
