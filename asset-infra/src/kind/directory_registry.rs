use asset_core::CoreError;
use asset_core::domain::{DirectoryKind, DirectoryKindDefinition};
use asset_core::port::DirectoryKindRegistry;
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

pub(super) fn directory_registry_from_builtin(
    catalog: &crate::builtin_catalog::BuiltinCatalog,
) -> Result<DefaultDirectoryKindRegistry, CoreError> {
    if catalog.directory_kinds.is_empty() {
        return Err(CoreError::configuration(
            "built-in directory kind catalog must not be empty",
        ));
    }
    Ok(DefaultDirectoryKindRegistry::from_definitions(
        catalog.directory_kinds.clone(),
    ))
}
