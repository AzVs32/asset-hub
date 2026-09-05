use super::directory_action_registry::push_directory_action_definition;
use super::*;
use crate::builtin_catalog::BuiltinCatalog;
use asset_core::CoreError;

/// All Kind and Action registries built from the Host-owned static catalog.
pub struct CapabilityCatalogs {
    pub resource_kinds: DefaultResourceKindRegistry,
    pub directory_kinds: DefaultDirectoryKindRegistry,
    pub resource_actions: DefaultResourceActionRegistry,
    pub directory_actions: DefaultDirectoryActionRegistry,
}

pub fn build_capability_catalogs(
    catalog: &BuiltinCatalog,
) -> Result<CapabilityCatalogs, CoreError> {
    let resource_kinds =
        DefaultResourceKindRegistry::from_definitions(catalog.resource_kinds.clone());
    let directory_kinds = directory_registry_from_builtin(catalog)?;

    let mut resource_actions = Vec::new();
    for action in &catalog.resource_actions {
        push_action_definition(
            &mut resource_actions,
            action.definition.clone(),
            "builtin:core.resource",
        )?;
    }
    let mut directory_actions = Vec::new();
    for action in &catalog.directory_actions {
        push_directory_action_definition(
            &mut directory_actions,
            action.definition.clone(),
            "builtin:core.directory",
        )?;
    }

    Ok(CapabilityCatalogs {
        resource_kinds,
        directory_kinds,
        resource_actions: DefaultResourceActionRegistry {
            actions: resource_actions,
        },
        directory_actions: DefaultDirectoryActionRegistry {
            actions: directory_actions,
        },
    })
}
