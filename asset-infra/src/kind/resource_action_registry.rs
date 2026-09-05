use asset_core::CoreError;
use asset_core::domain::ResourceActionDefinition;
use asset_core::port::ResourceActionRegistry;

use super::validation::ensure_unique_scoped_action;

/// Registry of Host-owned built-in Resource Actions.
#[derive(Debug, Clone, Default)]
pub struct DefaultResourceActionRegistry {
    pub(super) actions: Vec<ResourceActionDefinition>,
}

impl ResourceActionRegistry for DefaultResourceActionRegistry {
    fn actions(&self) -> &[ResourceActionDefinition] {
        &self.actions
    }
}

pub(super) fn push_action_definition(
    actions: &mut Vec<ResourceActionDefinition>,
    action: ResourceActionDefinition,
    source: &str,
) -> Result<(), CoreError> {
    ensure_unique_scoped_action(
        "resource",
        action.id().as_str(),
        action.kinds(),
        source,
        actions
            .iter()
            .map(|existing| (existing.id().as_str(), existing.kinds())),
    )?;
    actions.push(action);
    Ok(())
}
