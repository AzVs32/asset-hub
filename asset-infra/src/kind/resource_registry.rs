use super::validation::validate_hierarchy;
use asset_core::CoreError;
use asset_core::domain::{ResourceKind, ResourceKindDefinition};
use asset_core::port::ResourceKindRegistry;
use std::collections::HashMap;

/// 默认内置资源类型注册表。
///
/// 当前用于 MVP 阶段。后续插件系统接入后，可以替换为聚合插件定义的 registry 实现。
#[derive(Debug, Clone)]
pub struct DefaultResourceKindRegistry {
    pub(super) definitions: Vec<ResourceKindDefinition>,
    indices: HashMap<ResourceKind, usize>,
    pub(super) lineages: HashMap<ResourceKind, Vec<ResourceKind>>,
    descendants: HashMap<ResourceKind, Vec<ResourceKind>>,
}

impl DefaultResourceKindRegistry {
    pub(super) fn from_definitions(definitions: Vec<ResourceKindDefinition>) -> Self {
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

pub(super) fn validate_kind_hierarchy(
    definitions: &[ResourceKindDefinition],
) -> Result<(), CoreError> {
    validate_hierarchy(
        "resource",
        definitions
            .iter()
            .map(|definition| {
                (
                    definition.kind().as_str(),
                    definition.parent().map(|parent| parent.as_str()),
                )
            })
            .collect(),
    )
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
