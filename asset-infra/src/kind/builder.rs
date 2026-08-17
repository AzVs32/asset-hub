use super::directory_action_registry::{
    push_directory_action, push_directory_action_definition, validate_directory_action_capabilities,
};
use super::*;
use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::{
    DefinitionOrigin, ResourceActionDefinition, ResourceContentMatcher, ResourceKind,
    ResourceKindDefinition,
};
use asset_core::port::DirectoryKindRegistry;
use asset_plugin_sdk::manifest::ResourceKindCapability;

use super::normalization::content_matcher;
use super::validation::ensure_unique_id;

fn directory_action_registry_from_catalog(
    catalog: &PluginCatalog,
    kind_registry: &dyn DirectoryKindRegistry,
) -> Result<DefaultDirectoryActionRegistry, CoreError> {
    let mut actions = Vec::new();
    for action in &catalog.builtin.directory_actions {
        push_directory_action_definition(
            &mut actions,
            action.definition.clone(),
            "builtin:core.directory",
        )?;
    }
    for plugin in catalog.plugins() {
        for action in &plugin.manifest.capabilities.directory_actions {
            push_directory_action(
                &mut actions,
                action,
                DefinitionOrigin::plugin(plugin.manifest.plugin_id())?,
            )?;
        }
    }
    validate_directory_action_capabilities(kind_registry, &actions)?;
    Ok(DefaultDirectoryActionRegistry { actions })
}

/// All normalized kind and action registries built from one verified plugin catalog snapshot.
pub struct CapabilityCatalogs {
    pub resource_kinds: DefaultResourceKindRegistry,
    pub directory_kinds: DefaultDirectoryKindRegistry,
    pub resource_actions: DefaultResourceActionRegistry,
    pub directory_actions: DefaultDirectoryActionRegistry,
}

pub fn build_capability_catalogs(catalog: &PluginCatalog) -> Result<CapabilityCatalogs, CoreError> {
    let (definitions, actions) = build_registries_with_catalog(catalog)?;
    let resource_kinds = DefaultResourceKindRegistry::from_definitions(definitions);
    let directory_kinds = directory_registry_from_catalog(catalog)?;
    let resource_actions = DefaultResourceActionRegistry { actions };
    let directory_actions = directory_action_registry_from_catalog(catalog, &directory_kinds)?;
    Ok(CapabilityCatalogs {
        resource_kinds,
        directory_kinds,
        resource_actions,
        directory_actions,
    })
}

pub(super) fn build_registries_with_catalog(
    catalog: &PluginCatalog,
) -> Result<(Vec<ResourceKindDefinition>, Vec<ResourceActionDefinition>), CoreError> {
    let mut definitions = Vec::new();
    for definition in &catalog.builtin.resource_kinds {
        push_definition(&mut definitions, definition.clone())?;
    }
    for plugin in catalog.plugins() {
        for config_definition in &plugin.manifest.capabilities.resource_kinds {
            push_definition(
                &mut definitions,
                definition_from_manifest_kind(
                    config_definition,
                    DefinitionOrigin::plugin(plugin.manifest.plugin_id())?,
                )?,
            )?;
        }
    }
    validate_kind_hierarchy(&definitions)?;

    let mut actions = Vec::new();
    let mut inherited_action_labels = Vec::new();
    for action in &catalog.builtin.resource_actions {
        push_action_definition(
            &mut actions,
            action.definition.clone(),
            "builtin:core.resource",
        )?;
    }
    for plugin in catalog.plugins() {
        for action in &plugin.manifest.capabilities.resource_actions {
            let first_definition = actions.len();
            let action_definitions = action_definitions_with_inherited_content(
                &definitions,
                action,
                action.label.as_deref().unwrap_or(action.id.as_str()),
                DefinitionOrigin::plugin(plugin.manifest.plugin_id())?,
            )?;
            for action_definition in action_definitions {
                push_action_definition(
                    &mut actions,
                    action_definition,
                    format!("plugin:{}", plugin.manifest.plugin_id()),
                )?;
            }
            if action.label.is_none() {
                inherited_action_labels.extend(first_definition..actions.len());
            }
        }
    }
    resolve_inherited_resource_action_labels(&definitions, &mut actions, &inherited_action_labels)?;
    validate_resource_action_capabilities(&definitions, &actions)?;

    Ok((definitions, actions))
}

pub(super) fn push_definition(
    definitions: &mut Vec<ResourceKindDefinition>,
    definition: ResourceKindDefinition,
) -> Result<(), CoreError> {
    ensure_unique_id(
        "resource kind",
        definition.kind().as_str(),
        definitions.iter().map(|existing| existing.kind().as_str()),
    )?;

    definitions.push(definition);
    Ok(())
}

pub(super) fn definition_from_manifest_kind(
    config: &ResourceKindCapability,
    origin: DefinitionOrigin,
) -> Result<ResourceKindDefinition, CoreError> {
    let label = config.label.as_deref().unwrap_or(config.kind.as_str());
    definition_from_parts(
        &config.kind,
        label,
        config.parent.as_deref(),
        config.supports_content,
        content_matcher(&config.detect),
        origin,
    )
}

pub(super) fn definition_from_parts(
    kind: &str,
    label: &str,
    parent: Option<&str>,
    supports_content: bool,
    detect: ResourceContentMatcher,
    origin: DefinitionOrigin,
) -> Result<ResourceKindDefinition, CoreError> {
    Ok(ResourceKindDefinition::new(
        ResourceKind::try_new(kind)?,
        label,
        supports_content,
        origin,
    )
    .with_parent(parent.map(ResourceKind::try_new).transpose()?)
    .with_detect(detect))
}
