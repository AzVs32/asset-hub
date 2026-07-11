//! Resource action registry port.

use crate::domain::{Resource, ResourceKind};
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

    /// List actions whose applies_to model matches a concrete resource.
    fn actions_for_resource(&self, resource: &Resource) -> Vec<ResourceActionDefinition> {
        self.actions_for_resource_kinds(resource, std::slice::from_ref(resource.kind()))
    }

    fn actions_for_resource_kinds(
        &self,
        resource: &Resource,
        kinds: &[ResourceKind],
    ) -> Vec<ResourceActionDefinition> {
        let content = resource.content();
        self.actions()
            .iter()
            .filter(|action| {
                kinds.iter().any(|kind| {
                    action.matches_resource(
                        kind.as_str(),
                        content.and_then(|content| content.mime_type()),
                        content.map(|content| content.key().as_str()),
                    )
                })
            })
            .cloned()
            .collect()
    }
}
