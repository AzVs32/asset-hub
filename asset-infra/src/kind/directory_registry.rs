use super::validation::{ensure_unique_id, validate_hierarchy as validate_kind_hierarchy};
use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::{DefinitionOrigin, DirectoryKind, DirectoryKindDefinition};
use asset_core::port::DirectoryKindRegistry;
use asset_plugin_sdk::manifest::DirectoryKindCapability;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct DefaultDirectoryKindRegistry {
    definitions: Vec<DirectoryKindDefinition>,
    indices: HashMap<DirectoryKind, usize>,
    lineages: HashMap<DirectoryKind, Vec<DirectoryKind>>,
    descendants: HashMap<DirectoryKind, Vec<DirectoryKind>>,
}

impl DefaultDirectoryKindRegistry {
    fn from_definitions(definitions: Vec<DirectoryKindDefinition>) -> Self {
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
            for ancestor in &lineages[definition.kind()] {
                descendants
                    .get_mut(ancestor)
                    .expect("lineage kinds must be indexed")
                    .push(definition.kind().clone());
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

impl DirectoryKindRegistry for DefaultDirectoryKindRegistry {
    fn definitions(&self) -> &[DirectoryKindDefinition] {
        &self.definitions
    }

    fn get(&self, kind: &DirectoryKind) -> Option<&DirectoryKindDefinition> {
        self.indices
            .get(kind)
            .map(|index| &self.definitions[*index])
    }

    fn lineage(&self, kind: &DirectoryKind) -> Vec<DirectoryKind> {
        self.lineages.get(kind).cloned().unwrap_or_default()
    }

    fn descendants(&self, kind: &DirectoryKind) -> Vec<DirectoryKind> {
        self.descendants.get(kind).cloned().unwrap_or_default()
    }
}

pub(super) fn directory_registry_from_catalog(
    catalog: &PluginCatalog,
) -> Result<DefaultDirectoryKindRegistry, CoreError> {
    let mut definitions = Vec::new();
    for definition in &catalog.builtin.directory_kinds {
        push_definition(&mut definitions, definition.clone())?;
    }
    for plugin in catalog.plugins() {
        for capability in &plugin.manifest.capabilities.directory_kinds {
            push_definition(
                &mut definitions,
                definition_from_manifest(
                    capability,
                    DefinitionOrigin::plugin(plugin.manifest.plugin_id())?,
                )?,
            )?;
        }
    }
    validate_hierarchy(&definitions)?;
    Ok(DefaultDirectoryKindRegistry::from_definitions(definitions))
}

fn definition_from_manifest(
    capability: &DirectoryKindCapability,
    origin: DefinitionOrigin,
) -> Result<DirectoryKindDefinition, CoreError> {
    let label = capability
        .label
        .as_deref()
        .unwrap_or(capability.kind.as_str());
    Ok(
        DirectoryKindDefinition::new(DirectoryKind::try_new(&capability.kind)?, label, origin)
            .with_parent(
                capability
                    .parent
                    .as_deref()
                    .map(DirectoryKind::try_new)
                    .transpose()?,
            )
            .with_default_child_kind(
                capability
                    .default_child_kind
                    .as_deref()
                    .map(DirectoryKind::try_new)
                    .transpose()?,
            )
            .with_allowed_parent_kinds(
                capability
                    .allowed_parent_kinds
                    .iter()
                    .map(DirectoryKind::try_new)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
    )
}

fn push_definition(
    definitions: &mut Vec<DirectoryKindDefinition>,
    definition: DirectoryKindDefinition,
) -> Result<(), CoreError> {
    ensure_unique_id(
        "directory kind",
        definition.kind().as_str(),
        definitions.iter().map(|existing| existing.kind().as_str()),
    )?;
    definitions.push(definition);
    Ok(())
}

pub(super) fn validate_hierarchy(definitions: &[DirectoryKindDefinition]) -> Result<(), CoreError> {
    validate_kind_hierarchy(
        "directory",
        definitions
            .iter()
            .map(|definition| {
                (
                    definition.kind().as_str(),
                    definition.parent().map(DirectoryKind::as_str),
                )
            })
            .collect(),
    )?;
    for definition in definitions {
        if let Some(default_child_kind) = definition.default_child_kind() {
            let Some(default_definition) = definitions
                .iter()
                .find(|candidate| candidate.kind() == default_child_kind)
            else {
                return Err(CoreError::configuration(format!(
                    "directory kind `{}` declares unknown default child kind `{default_child_kind}`",
                    definition.kind()
                )));
            };
            if !definition_is_a(definitions, default_child_kind, definition.kind()) {
                return Err(CoreError::configuration(format!(
                    "default child kind `{default_child_kind}` must inherit from `{}`",
                    definition.kind()
                )));
            }
            let allowed_parent_kinds =
                effective_allowed_parent_kinds(definitions, default_definition);
            if !allowed_parent_kinds.is_empty()
                && !allowed_parent_kinds
                    .iter()
                    .any(|allowed| definition_is_a(definitions, definition.kind(), allowed))
            {
                return Err(CoreError::configuration(format!(
                    "default child kind `{default_child_kind}` does not allow parent kind `{}`",
                    definition.kind()
                )));
            }
        }
        for parent in definition.allowed_parent_kinds() {
            if !definitions
                .iter()
                .any(|candidate| candidate.kind() == parent)
            {
                return Err(CoreError::configuration(format!(
                    "directory kind `{}` allows unknown parent kind `{parent}`",
                    definition.kind()
                )));
            }
        }
    }
    Ok(())
}

fn definition_is_a(
    definitions: &[DirectoryKindDefinition],
    kind: &DirectoryKind,
    ancestor: &DirectoryKind,
) -> bool {
    let mut current = Some(kind);
    while let Some(kind) = current {
        if kind == ancestor {
            return true;
        }
        current = definitions
            .iter()
            .find(|definition| definition.kind() == kind)
            .and_then(DirectoryKindDefinition::parent);
    }
    false
}

fn effective_allowed_parent_kinds<'a>(
    definitions: &'a [DirectoryKindDefinition],
    definition: &'a DirectoryKindDefinition,
) -> &'a [DirectoryKind] {
    let mut current = Some(definition);
    while let Some(definition) = current {
        if !definition.allowed_parent_kinds().is_empty() {
            return definition.allowed_parent_kinds();
        }
        current = definition.parent().and_then(|parent| {
            definitions
                .iter()
                .find(|candidate| candidate.kind() == parent)
        });
    }
    &[]
}
