use super::directory_action_registry::{push_directory_action, push_directory_action_definition};
use super::*;
use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::{ResourceActionDefinition, ResourceContentMatcher, ResourceKind};
use asset_core::port::ResourceKindDefinition;
use asset_plugin_api::manifest::ResourceKindCapability;

use super::normalization::content_matcher;

pub fn directory_action_registry_from_catalog(
    catalog: &PluginCatalog,
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
                &format!("plugin:{}", plugin.manifest.plugin_id()),
            )?;
        }
    }
    Ok(DefaultDirectoryActionRegistry { actions })
}

pub fn registries_from_catalog(
    catalog: &PluginCatalog,
) -> Result<
    (
        DefaultResourceKindRegistry,
        DefaultDirectoryKindRegistry,
        DefaultResourceActionRegistry,
    ),
    CoreError,
> {
    let (definitions, actions) = build_registries_with_catalog(catalog)?;
    Ok((
        DefaultResourceKindRegistry::from_definitions(definitions),
        directory_registry_from_catalog(catalog)?,
        DefaultResourceActionRegistry { actions },
    ))
}

pub(super) fn build_registries_with_catalog(
    catalog: &PluginCatalog,
) -> Result<(Vec<ResourceKindDefinition>, Vec<ResourceActionDefinition>), CoreError> {
    let mut definitions = Vec::new();
    for definition in &catalog.builtin.resource_kinds {
        push_definition(&mut definitions, definition.clone())?;
    }
    for plugin in catalog.plugins() {
        for config_definition in &plugin.manifest.capabilities.kinds {
            push_definition(
                &mut definitions,
                definition_from_manifest_kind(
                    config_definition,
                    format!("plugin:{}", plugin.manifest.plugin_id()),
                )?,
            )?;
        }
    }
    validate_kind_hierarchy(&definitions)?;

    let mut actions = Vec::new();
    for action in &catalog.builtin.resource_actions {
        push_action_definition(
            &mut actions,
            action.definition.clone(),
            "builtin:core.resource",
        )?;
    }
    for plugin in catalog.plugins() {
        for action in &plugin.manifest.capabilities.resource_actions {
            let action_definitions =
                action_definitions_with_inherited_content(&definitions, action)?;
            for action_definition in action_definitions {
                push_action_definition(
                    &mut actions,
                    action_definition,
                    format!("plugin:{}", plugin.manifest.plugin_id()),
                )?;
            }
        }
    }

    Ok((definitions, actions))
}

pub(super) fn push_definition(
    definitions: &mut Vec<ResourceKindDefinition>,
    definition: ResourceKindDefinition,
) -> Result<(), CoreError> {
    if definitions
        .iter()
        .any(|existing| existing.kind().as_str() == definition.kind().as_str())
    {
        return Err(CoreError::configuration(format!(
            "duplicate resource kind `{}`",
            definition.kind()
        )));
    }

    definitions.push(definition);
    Ok(())
}

pub(super) fn definition_from_manifest_kind(
    config: &ResourceKindCapability,
    source: impl Into<String>,
) -> Result<ResourceKindDefinition, CoreError> {
    let label = config.label.as_deref().unwrap_or(config.kind.as_str());
    definition_from_parts(
        &config.kind,
        label,
        config.parent.as_deref(),
        config.supports_content,
        content_matcher(&config.detect),
        source,
    )
}

pub(super) fn definition_from_parts(
    kind: &str,
    label: &str,
    parent: Option<&str>,
    supports_content: bool,
    detect: ResourceContentMatcher,
    source: impl Into<String>,
) -> Result<ResourceKindDefinition, CoreError> {
    Ok(ResourceKindDefinition::with_source(
        ResourceKind::try_new(kind)?,
        label,
        supports_content,
        source,
    )
    .with_parent(parent.map(ResourceKind::try_new).transpose()?)
    .with_detect(detect))
}
