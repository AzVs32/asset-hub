use asset_core::{CoreError, domain::DirectoryActionDefinition, port::DirectoryActionRegistry};
use asset_plugin_api::manifest::DirectoryActionCapability;

use super::normalization::directory_action_definition;

#[derive(Debug, Clone, Default)]
pub struct DefaultDirectoryActionRegistry {
    pub(super) actions: Vec<DirectoryActionDefinition>,
}

impl DirectoryActionRegistry for DefaultDirectoryActionRegistry {
    fn actions(&self) -> &[DirectoryActionDefinition] {
        &self.actions
    }
}

pub(super) fn push_directory_action(
    actions: &mut Vec<DirectoryActionDefinition>,
    capability: &DirectoryActionCapability,
    source: &str,
) -> Result<(), CoreError> {
    let action = directory_action_definition(capability);
    push_directory_action_definition(actions, action, source)
}

pub(super) fn push_directory_action_definition(
    actions: &mut Vec<DirectoryActionDefinition>,
    action: DirectoryActionDefinition,
    source: &str,
) -> Result<(), CoreError> {
    if actions.iter().any(|existing| {
        existing.id() == action.id()
            && (existing.kinds().is_empty()
                || action.kinds().is_empty()
                || existing.kinds().iter().any(|kind| {
                    action
                        .kinds()
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(kind))
                }))
    }) {
        return Err(CoreError::configuration(format!(
            "duplicate global directory action `{}` from {source}",
            action.id()
        )));
    }
    actions.push(action);
    Ok(())
}
