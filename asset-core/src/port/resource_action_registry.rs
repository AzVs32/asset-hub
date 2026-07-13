//! Resource action registry port.

use crate::domain::ResourceKind;
pub use asset_plugin_api::{ResourceAction, ResourceActionDefinition};

/// Registry for globally contributed resource actions.
pub trait ResourceActionRegistry: Send + Sync {
    /// List every action capability currently available in the runtime.
    fn actions(&self) -> &[ResourceActionDefinition];

    /// Find an action by id.
    fn get_action(&self, action: &ResourceAction) -> Option<ResourceActionDefinition> {
        self.actions()
            .iter()
            .find(|definition| definition.id().as_str() == action.as_str())
            .cloned()
    }

    /// List actions declared for one of the resource kind lineage entries.
    fn actions_for_kinds(&self, kinds: &[ResourceKind]) -> Vec<ResourceActionDefinition> {
        self.actions()
            .iter()
            .filter(|action| {
                action.kinds().is_empty()
                    || kinds.iter().any(|kind| {
                        action
                            .kinds()
                            .iter()
                            .any(|expected| expected.eq_ignore_ascii_case(kind.as_str()))
                    })
            })
            .cloned()
            .collect()
    }
}
