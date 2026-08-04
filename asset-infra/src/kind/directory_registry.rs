use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::DirectoryKind;
use asset_core::port::{DirectoryKindDefinition, DirectoryKindRegistry};
use asset_plugin_api::manifest::DirectoryKindCapability;
use std::collections::{HashMap, HashSet};

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
                    format!("plugin:{}", plugin.manifest.plugin_id()),
                )?,
            )?;
        }
    }
    validate_hierarchy(&definitions)?;
    Ok(DefaultDirectoryKindRegistry::from_definitions(definitions))
}

fn definition_from_manifest(
    capability: &DirectoryKindCapability,
    source: impl Into<String>,
) -> Result<DirectoryKindDefinition, CoreError> {
    let label = capability
        .label
        .as_deref()
        .unwrap_or(capability.kind.as_str());
    Ok(DirectoryKindDefinition::with_source(
        DirectoryKind::try_new(&capability.kind)?,
        label,
        source,
    )
    .with_parent(
        capability
            .parent
            .as_deref()
            .map(DirectoryKind::try_new)
            .transpose()?,
    ))
}

fn push_definition(
    definitions: &mut Vec<DirectoryKindDefinition>,
    definition: DirectoryKindDefinition,
) -> Result<(), CoreError> {
    if definitions
        .iter()
        .any(|existing| existing.kind() == definition.kind())
    {
        return Err(CoreError::configuration(format!(
            "duplicate directory kind `{}`",
            definition.kind()
        )));
    }
    definitions.push(definition);
    Ok(())
}

fn validate_hierarchy(definitions: &[DirectoryKindDefinition]) -> Result<(), CoreError> {
    let parents = definitions
        .iter()
        .map(|definition| {
            (
                definition.kind().as_str(),
                definition.parent().map(DirectoryKind::as_str),
            )
        })
        .collect::<HashMap<_, _>>();

    for definition in definitions {
        let mut current = Some(definition.kind().as_str());
        let mut visited = HashSet::new();
        while let Some(kind) = current {
            if !visited.insert(kind) {
                return Err(CoreError::configuration(format!(
                    "directory kind hierarchy contains a cycle at `{kind}`"
                )));
            }
            let Some(parent) = parents.get(kind) else {
                return Err(CoreError::configuration(format!(
                    "directory kind `{}` references unknown parent `{kind}`",
                    definition.kind()
                )));
            };
            current = *parent;
        }
    }
    Ok(())
}
