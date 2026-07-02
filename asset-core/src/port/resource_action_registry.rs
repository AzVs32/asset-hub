//! Resource action registry port.

use crate::domain::Resource;
pub use asset_plugin_api::{ResourceAction, ResourceActionDefinition};

/// Registry for globally contributed resource actions.
pub trait ResourceActionRegistry: Send + Sync {
    /// List every action capability currently available in the runtime.
    fn list_actions(&self) -> Vec<ResourceActionDefinition>;

    /// Find an action by id.
    fn get_action(&self, action: &ResourceAction) -> Option<ResourceActionDefinition> {
        self.list_actions()
            .into_iter()
            .find(|definition| definition.id().as_str() == action.as_str())
    }

    /// List actions whose applies_to model matches a concrete resource.
    fn actions_for_resource(&self, resource: &Resource) -> Vec<ResourceActionDefinition> {
        let content = resource.content();
        self.list_actions()
            .into_iter()
            .filter(|action| {
                action.matches_resource(
                    resource.kind().as_str(),
                    content.and_then(|content| content.mime_type()),
                    content.map(|content| content.key().as_str()),
                )
            })
            .collect()
    }
}
