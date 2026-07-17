use crate::config::{KindRegistryConfig, ResourceKindConfig};
use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::ResourceKind;
use asset_core::port::{ResourceActionRegistry, ResourceKindDefinition, ResourceKindRegistry};
use asset_plugin_api::{
    PluginRuntime, ResourceActionCapability, ResourceActionDefinition, ResourceContentMatcher,
    ResourceKindCapability,
};
use std::collections::{HashMap, HashSet};

/// 默认内置资源类型注册表。
///
/// 当前用于 MVP 阶段。后续插件系统接入后，可以替换为聚合插件定义的 registry 实现。
#[derive(Debug, Clone)]
pub(crate) struct DefaultResourceKindRegistry {
    definitions: Vec<ResourceKindDefinition>,
    indices: HashMap<ResourceKind, usize>,
    lineages: HashMap<ResourceKind, Vec<ResourceKind>>,
    descendants: HashMap<ResourceKind, Vec<ResourceKind>>,
}

/// 默认资源动作注册表。
#[derive(Debug, Clone)]
pub(crate) struct DefaultResourceActionRegistry {
    actions: Vec<ResourceActionDefinition>,
}

impl DefaultResourceKindRegistry {
    fn from_definitions(definitions: Vec<ResourceKindDefinition>) -> Self {
        let indices = definitions
            .iter()
            .enumerate()
            .map(|(index, definition)| (definition.kind().clone(), index))
            .collect::<HashMap<_, _>>();
        let mut lineages = HashMap::with_capacity(definitions.len());
        for definition in &definitions {
            let mut lineage = Vec::new();
            let mut current = Some(definition.kind());
            while let Some(kind) = current {
                lineage.push(kind.clone());
                current = indices
                    .get(kind)
                    .and_then(|index| definitions[*index].parent());
            }
            lineages.insert(definition.kind().clone(), lineage);
        }
        let mut descendants = definitions
            .iter()
            .map(|definition| (definition.kind().clone(), Vec::new()))
            .collect::<HashMap<_, _>>();
        for definition in &definitions {
            let kind = definition.kind();
            let lineage = &lineages[kind];
            for ancestor in lineage {
                descendants
                    .get_mut(ancestor)
                    .expect("lineage kinds must be indexed")
                    .push(kind.clone());
            }
        }

        Self {
            definitions,
            indices,
            lineages,
            descendants,
        }
    }
}

pub(crate) fn registries_from_catalog(
    config: &KindRegistryConfig,
    catalog: &PluginCatalog,
) -> Result<(DefaultResourceKindRegistry, DefaultResourceActionRegistry), CoreError> {
    let (definitions, actions) = build_registries_with_catalog(config, catalog)?;
    Ok((
        DefaultResourceKindRegistry::from_definitions(definitions),
        DefaultResourceActionRegistry { actions },
    ))
}

fn build_registries_with_catalog(
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

    for kind in ResourceKind::builtin_values() {
        let parent = (*kind == ResourceKind::UNKNOWN).then_some("core:file");
        push_definition(
            &mut definitions,
            definition_from_parts(
                kind,
                kind,
                parent,
                true,
                ResourceContentMatcher::default(),
                "builtin",
            )?,
        )?;
    }
    for manifest in &official_manifests {
        for config_definition in &manifest.manifest.capabilities.resource_kinds {
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
        for config_definition in &manifest.manifest.capabilities.resource_kinds {
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
        for action in &manifest.manifest.capabilities.resource_actions {
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
        for action in &manifest.manifest.capabilities.resource_actions {
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

fn validate_kind_hierarchy(definitions: &[ResourceKindDefinition]) -> Result<(), CoreError> {
    let parents = definitions
        .iter()
        .map(|definition| {
            (
                definition.kind().as_str(),
                definition.parent().map(|parent| parent.as_str()),
            )
        })
        .collect::<HashMap<_, _>>();

    for definition in definitions {
        let mut current = Some(definition.kind().as_str());
        let mut visited = HashSet::new();
        while let Some(kind) = current {
            if !visited.insert(kind) {
                return Err(CoreError::configuration(format!(
                    "resource kind hierarchy contains a cycle at `{kind}`"
                )));
            }
            let Some(parent) = parents.get(kind) else {
                return Err(CoreError::configuration(format!(
                    "resource kind `{}` references unknown parent `{kind}`",
                    definition.kind()
                )));
            };
            current = *parent;
        }
    }
    Ok(())
}

impl ResourceKindRegistry for DefaultResourceKindRegistry {
    fn definitions(&self) -> &[ResourceKindDefinition] {
        &self.definitions
    }

    fn get(&self, kind: &ResourceKind) -> Option<&ResourceKindDefinition> {
        self.indices
            .get(kind)
            .map(|index| &self.definitions[*index])
    }

    fn lineage(&self, kind: &ResourceKind) -> Vec<ResourceKind> {
        self.lineages.get(kind).cloned().unwrap_or_default()
    }

    fn descendants(&self, kind: &ResourceKind) -> Vec<ResourceKind> {
        self.descendants.get(kind).cloned().unwrap_or_default()
    }
}

impl ResourceActionRegistry for DefaultResourceActionRegistry {
    fn actions(&self) -> &[ResourceActionDefinition] {
        &self.actions
    }
}

fn push_definition(
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

fn action_definitions_with_inherited_content(
    definitions: &[ResourceKindDefinition],
    action: &ResourceActionCapability,
    runtime: &PluginRuntime,
) -> Result<Vec<ResourceActionDefinition>, CoreError> {
    let definition = action.to_definition(runtime);
    if !should_inherit_detect_for_action(action) || !definition.content_matcher().is_empty() {
        return Ok(vec![definition]);
    }
    action
        .applies_to
        .kinds
        .iter()
        .map(|kind| {
            Ok(definition
                .clone()
                .with_kinds([kind.clone()])
                .with_content_matcher(detect_for_kind(definitions, kind)?))
        })
        .collect()
}

fn should_inherit_detect_for_action(action: &ResourceActionCapability) -> bool {
    action.applies_to.kinds.len() > 1
}

fn detect_for_kind(
    definitions: &[ResourceKindDefinition],
    kind: &str,
) -> Result<ResourceContentMatcher, CoreError> {
    definitions
        .iter()
        .find(|definition| definition.kind().as_str().eq_ignore_ascii_case(kind))
        .map(|definition| definition.detect().clone())
        .ok_or_else(|| {
            CoreError::configuration(format!("resource action references unknown kind `{kind}`"))
        })
}

fn push_action_definition(
    actions: &mut Vec<ResourceActionDefinition>,
    action: ResourceActionDefinition,
    source: impl Into<String>,
) -> Result<(), CoreError> {
    let source = source.into();
    if actions.iter().any(|existing| {
        existing.id().as_str() == action.id().as_str()
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
            "duplicate global resource action `{}` from {source}",
            action.id()
        )));
    }

    actions.push(action);
    Ok(())
}

fn definition_from_manifest_kind(
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

fn definition_from_config(
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

fn definition_from_parts(
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

#[cfg(test)]
mod tests;
