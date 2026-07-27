use super::directory_action_registry::push_directory_action;
use super::*;
use crate::config::{KindRegistryConfig, ResourceKindConfig};
use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::ResourceKind;
use asset_core::port::ResourceKindDefinition;
use asset_plugin_api::{ResourceActionDefinition, ResourceContentMatcher, ResourceKindCapability};

pub(crate) fn directory_action_registry_from_catalog(
    catalog: &PluginCatalog,
) -> Result<DefaultDirectoryActionRegistry, CoreError> {
    let mut actions = Vec::new();
    for plugin in catalog.plugins() {
        for action in &plugin.manifest.capabilities.directory_actions {
            push_directory_action(
                &mut actions,
                action,
                &plugin.manifest.runtime,
                &format!("plugin:{}", plugin.manifest.plugin_id()),
            )?;
        }
    }
    Ok(DefaultDirectoryActionRegistry { actions })
}

pub(crate) fn registries_from_catalog(
    config: &KindRegistryConfig,
    catalog: &PluginCatalog,
) -> Result<
    (
        DefaultResourceKindRegistry,
        DefaultDirectoryKindRegistry,
        DefaultResourceActionRegistry,
    ),
    CoreError,
> {
    let (definitions, actions) = build_registries_with_catalog(config, catalog)?;
    Ok((
        DefaultResourceKindRegistry::from_definitions(definitions),
        directory_registry_from_catalog(catalog)?,
        DefaultResourceActionRegistry { actions },
    ))
}

pub(super) fn build_registries_with_catalog(
    config: &KindRegistryConfig,
    catalog: &PluginCatalog,
) -> Result<(Vec<ResourceKindDefinition>, Vec<ResourceActionDefinition>), CoreError> {
    let mut definitions = Vec::new();
    let official_manifests = catalog
        .plugins()
        .iter()
        .filter(|plugin| plugin.manifest_path.is_none())
        .collect::<Vec<_>>();
    let plugin_manifests = catalog
        .plugins()
        .iter()
        .filter(|plugin| plugin.manifest_path.is_some())
        .collect::<Vec<_>>();

    for manifest in &official_manifests {
        for config_definition in &manifest.manifest.capabilities.kinds {
            push_definition(
                &mut definitions,
                definition_from_manifest_kind(
                    config_definition,
                    format!("plugin:{}", manifest.manifest.plugin_id()),
                )?,
            )?;
        }
    }

    for config_definition in &config.definitions {
        push_definition(
            &mut definitions,
            definition_from_config(config_definition, "config")?,
        )?;
    }

    for manifest in &plugin_manifests {
        for config_definition in &manifest.manifest.capabilities.kinds {
            push_definition(
                &mut definitions,
                definition_from_manifest_kind(
                    config_definition,
                    format!("plugin:{}", manifest.manifest.plugin_id()),
                )?,
            )?;
        }
    }
    validate_kind_hierarchy(&definitions)?;

    let mut actions = Vec::new();
    for definition in &config.definitions {
        for action in &definition.actions {
            push_action_definition(
                &mut actions,
                action.clone().with_kinds([definition.kind.clone()]),
                format!("config kind `{}`", definition.kind),
            )?;
        }
    }
    for manifest in &official_manifests {
        for action in &manifest.manifest.capabilities.actions {
            let action_definitions = action_definitions_with_inherited_content(
                &definitions,
                action,
                &manifest.manifest.runtime,
            )?;
            for action_definition in action_definitions {
                push_action_definition(
                    &mut actions,
                    action_definition,
                    format!("plugin:{}", manifest.manifest.plugin_id()),
                )?;
            }
        }
    }
    for manifest in &plugin_manifests {
        for action in &manifest.manifest.capabilities.actions {
            let action_definitions = action_definitions_with_inherited_content(
                &definitions,
                action,
                &manifest.manifest.runtime,
            )?;
            for action_definition in action_definitions {
                push_action_definition(
                    &mut actions,
                    action_definition,
                    format!("plugin:{}", manifest.manifest.plugin_id()),
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
        config.detect.clone(),
        source,
    )
}

pub(super) fn definition_from_config(
    config: &ResourceKindConfig,
    source: impl Into<String>,
) -> Result<ResourceKindDefinition, CoreError> {
    let label = config.label.as_deref().unwrap_or(config.kind.as_str());
    definition_from_parts(
        &config.kind,
        label,
        config.parent.as_deref(),
        config.supports_content,
        config.detect.clone(),
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
