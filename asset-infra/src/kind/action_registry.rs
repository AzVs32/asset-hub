use asset_core::CoreError;
use asset_core::domain::{
    ActionAccess, DefinitionOrigin, ResourceActionDefinition, ResourceContentMatcher,
    ResourceKindDefinition,
};
use asset_core::port::ResourceActionRegistry;
use asset_plugin_api::manifest::ResourceActionCapability;

use super::normalization::resource_action_definition;
use super::validation::ensure_unique_scoped_action;
use crate::builtin_catalog::THUMBNAIL_CAPABILITY;

const RESOURCE_THUMBNAIL_LOCATION: &str = "resource_thumbnail";
const TEXT_EDIT_CAPABILITY: &str = "text_edit";
const RESOURCE_CAPABILITIES: &[&str] = &[THUMBNAIL_CAPABILITY, TEXT_EDIT_CAPABILITY];

/// 默认资源动作注册表。
#[derive(Debug, Clone)]
pub struct DefaultResourceActionRegistry {
    pub(super) actions: Vec<ResourceActionDefinition>,
}

impl ResourceActionRegistry for DefaultResourceActionRegistry {
    fn actions(&self) -> &[ResourceActionDefinition] {
        &self.actions
    }
}

pub(super) fn action_definitions_with_inherited_content(
    definitions: &[ResourceKindDefinition],
    action: &ResourceActionCapability,
    label: &str,
    origin: DefinitionOrigin,
) -> Result<Vec<ResourceActionDefinition>, CoreError> {
    let definition = resource_action_definition(action, label, origin);
    let split_for_label = action.label.is_none() && action.applies_to.kinds.len() > 1;
    let inherit_content =
        should_inherit_detect_for_action(action) && definition.content_matcher().is_empty();
    if !split_for_label && !inherit_content {
        return Ok(vec![definition]);
    }
    action
        .applies_to
        .kinds
        .iter()
        .map(|kind| {
            let definition = definition.clone().with_kinds([kind.clone()]);
            if inherit_content {
                Ok(definition.with_content_matcher(detect_for_kind(definitions, kind)?))
            } else {
                Ok(definition)
            }
        })
        .collect()
}

/// Replace omitted plugin labels with the nearest ancestor provider's normalized label.
pub(super) fn resolve_inherited_resource_action_labels(
    definitions: &[ResourceKindDefinition],
    actions: &mut [ResourceActionDefinition],
    inherited_indices: &[usize],
) -> Result<(), CoreError> {
    let mut ordered = inherited_indices.to_vec();
    ordered.sort_by_key(|index| {
        action_kind_lineage(definitions, &actions[*index])
            .map_or(usize::MAX, |lineage| lineage.len())
    });

    for index in ordered {
        let action = &actions[index];
        let capability = action.provides().ok_or_else(|| {
            CoreError::configuration(format!(
                "resource action `{}` cannot inherit a label without providing a capability",
                action.id()
            ))
        })?;
        let lineage = action_kind_lineage(definitions, action)?;
        let label = inherited_label_from_lineage(actions, index, capability.as_str(), &lineage)?;
        actions[index] = actions[index].clone().with_label(label);
    }
    Ok(())
}

fn action_kind_lineage<'a>(
    definitions: &'a [ResourceKindDefinition],
    action: &ResourceActionDefinition,
) -> Result<Vec<&'a str>, CoreError> {
    let [kind] = action.kinds() else {
        return Err(CoreError::configuration(format!(
            "resource action `{}` must target exactly one kind to inherit a label",
            action.id()
        )));
    };
    let definition = definitions
        .iter()
        .find(|definition| definition.kind().as_str() == kind)
        .ok_or_else(|| {
            CoreError::configuration(format!(
                "resource action `{}` references unknown kind `{kind}`",
                action.id()
            ))
        })?;
    Ok(resource_kind_lineage(definitions, definition))
}

fn inherited_label_from_lineage(
    actions: &[ResourceActionDefinition],
    action_index: usize,
    capability: &str,
    lineage: &[&str],
) -> Result<String, CoreError> {
    for ancestor in lineage.iter().skip(1) {
        let providers = actions
            .iter()
            .enumerate()
            .filter(|(index, action)| {
                *index != action_index
                    && action
                        .provides()
                        .is_some_and(|provided| provided.as_str() == capability)
                    && action.kinds().iter().any(|kind| kind.as_str() == *ancestor)
            })
            .map(|(_, action)| action)
            .collect::<Vec<_>>();
        match providers.as_slice() {
            [] => {}
            [provider] => return Ok(provider.label().to_string()),
            _ => {
                return Err(CoreError::configuration(format!(
                    "resource capability `{capability}` has multiple label providers on ancestor kind `{ancestor}`"
                )));
            }
        }
    }

    let global = actions
        .iter()
        .enumerate()
        .filter(|(index, action)| {
            *index != action_index
                && action
                    .provides()
                    .is_some_and(|provided| provided.as_str() == capability)
                && action.kinds().is_empty()
        })
        .map(|(_, action)| action)
        .collect::<Vec<_>>();
    match global.as_slice() {
        [provider] => Ok(provider.label().to_string()),
        [] => Err(CoreError::configuration(format!(
            "resource action cannot inherit `{capability}` label because no ancestor provider exists"
        ))),
        _ => Err(CoreError::configuration(format!(
            "resource capability `{capability}` has multiple global label providers"
        ))),
    }
}

pub(super) fn should_inherit_detect_for_action(action: &ResourceActionCapability) -> bool {
    action.applies_to.kinds.len() > 1
}

pub(super) fn detect_for_kind(
    definitions: &[ResourceKindDefinition],
    kind: &str,
) -> Result<ResourceContentMatcher, CoreError> {
    definitions
        .iter()
        .find(|definition| definition.kind().as_str() == kind)
        .map(|definition| definition.detect().clone())
        .ok_or_else(|| {
            CoreError::configuration(format!("resource action references unknown kind `{kind}`"))
        })
}

pub(super) fn push_action_definition(
    actions: &mut Vec<ResourceActionDefinition>,
    action: ResourceActionDefinition,
    source: impl Into<String>,
) -> Result<(), CoreError> {
    let source = source.into();
    ensure_unique_scoped_action(
        "resource",
        action.id().as_str(),
        action.kinds(),
        &source,
        actions
            .iter()
            .map(|existing| (existing.id().as_str(), existing.kinds())),
    )?;

    actions.push(action);
    Ok(())
}

pub(super) fn validate_resource_action_capabilities(
    definitions: &[ResourceKindDefinition],
    actions: &[ResourceActionDefinition],
) -> Result<(), CoreError> {
    for action in actions {
        if let Some(capability) = action
            .provides()
            .filter(|capability| !RESOURCE_CAPABILITIES.contains(&capability.as_str()))
        {
            return Err(CoreError::configuration(format!(
                "resource action `{}` provides unsupported capability `{capability}`",
                action.id(),
            )));
        }
        let in_thumbnail_slot = action
            .ui()
            .locations
            .iter()
            .any(|location| location == RESOURCE_THUMBNAIL_LOCATION);
        let provides_thumbnail = action
            .provides()
            .is_some_and(|capability| capability.as_str() == THUMBNAIL_CAPABILITY);
        if in_thumbnail_slot != provides_thumbnail {
            return Err(CoreError::configuration(format!(
                "resource action `{}` must pair `{RESOURCE_THUMBNAIL_LOCATION}` with capability `{THUMBNAIL_CAPABILITY}`",
                action.id()
            )));
        }
        if provides_thumbnail
            && (action.access() != ActionAccess::Read
                || !action.output().views.iter().any(|view| view == "media"))
        {
            return Err(CoreError::configuration(format!(
                "resource thumbnail provider `{}` must be read-only and support the `media` view",
                action.id()
            )));
        }
        let provides_text_edit = action
            .provides()
            .is_some_and(|capability| capability.as_str() == TEXT_EDIT_CAPABILITY);
        if provides_text_edit && action.access() != ActionAccess::Write {
            return Err(CoreError::configuration(format!(
                "resource text edit provider `{}` must have write access",
                action.id()
            )));
        }
    }
    for definition in definitions {
        let lineage = resource_kind_lineage(definitions, definition);
        for capability in RESOURCE_CAPABILITIES {
            validate_nearest_resource_capability_provider(
                definition.kind().as_str(),
                &lineage,
                actions,
                capability,
            )?;
        }
    }
    Ok(())
}

fn validate_nearest_resource_capability_provider(
    kind: &str,
    lineage: &[&str],
    actions: &[ResourceActionDefinition],
    capability: &str,
) -> Result<(), CoreError> {
    let mut nearest = None;
    let mut providers = Vec::new();
    for action in actions.iter().filter(|action| {
        action
            .provides()
            .is_some_and(|provided| provided.as_str() == capability)
    }) {
        let distance = if action.kinds().is_empty() {
            usize::MAX
        } else if let Some(distance) = lineage.iter().position(|kind| {
            action
                .kinds()
                .iter()
                .any(|declared| declared.eq_ignore_ascii_case(kind))
        }) {
            distance
        } else {
            continue;
        };
        match nearest {
            None => {
                nearest = Some(distance);
                providers.push(action.id().as_str());
            }
            Some(current) if distance < current => {
                nearest = Some(distance);
                providers.clear();
                providers.push(action.id().as_str());
            }
            Some(current) if distance == current => providers.push(action.id().as_str()),
            Some(_) => {}
        }
    }
    if providers.len() > 1 {
        return Err(CoreError::configuration(format!(
            "resource kind `{kind}` has multiple nearest `{capability}` providers: {}",
            providers.join(", ")
        )));
    }
    Ok(())
}

fn resource_kind_lineage<'a>(
    definitions: &'a [ResourceKindDefinition],
    definition: &'a ResourceKindDefinition,
) -> Vec<&'a str> {
    let mut lineage = vec![definition.kind().as_str()];
    let mut parent = definition.parent();
    while let Some(kind) = parent {
        lineage.push(kind.as_str());
        parent = definitions
            .iter()
            .find(|candidate| candidate.kind() == kind)
            .and_then(ResourceKindDefinition::parent);
    }
    lineage
}
