use asset_core::{CoreError, domain::DirectoryActionDefinition, port::DirectoryActionRegistry};

use super::validation::ensure_unique_scoped_action;

/// Registry of Host-owned built-in Directory Actions.
#[derive(Debug, Clone, Default)]
pub struct DefaultDirectoryActionRegistry {
    pub(super) actions: Vec<DirectoryActionDefinition>,
}

impl DirectoryActionRegistry for DefaultDirectoryActionRegistry {
    fn actions(&self) -> &[DirectoryActionDefinition] {
        &self.actions
    }
}

pub(super) fn push_directory_action_definition(
    actions: &mut Vec<DirectoryActionDefinition>,
    action: DirectoryActionDefinition,
    source: &str,
) -> Result<(), CoreError> {
    ensure_unique_scoped_action(
        "directory",
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
