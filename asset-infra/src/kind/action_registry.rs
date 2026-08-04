use asset_core::CoreError;
use asset_core::domain::{ActionAccess, ResourceActionDefinition, ResourceContentMatcher};
use asset_core::port::{ResourceActionRegistry, ResourceKindDefinition};
use asset_plugin_api::manifest::ResourceActionCapability;

use super::normalization::resource_action_definition;
use crate::builtin_catalog::THUMBNAIL_CAPABILITY;

const RESOURCE_THUMBNAIL_LOCATION: &str = "resource_list_thumbnail";

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
) -> Result<Vec<ResourceActionDefinition>, CoreError> {
    let definition = resource_action_definition(action);
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

pub(super) fn should_inherit_detect_for_action(action: &ResourceActionCapability) -> bool {
    action.applies_to.kinds.len() > 1
}

pub(super) fn detect_for_kind(
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

pub(super) fn push_action_definition(
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

pub(super) fn validate_resource_action_capabilities(
    definitions: &[ResourceKindDefinition],
    actions: &[ResourceActionDefinition],
) -> Result<(), CoreError> {
    for action in actions {
        if let Some(capability) = action
            .provides()
            .filter(|capability| capability.as_str() != THUMBNAIL_CAPABILITY)
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
            && (action.access() != ActionAccess::ReadOnly
                || !action.output().view.iter().any(|view| view == "media"))
        {
            return Err(CoreError::configuration(format!(
                "resource thumbnail provider `{}` must be read-only and support the `media` view",
                action.id()
            )));
        }
    }
    for definition in definitions {
        let lineage = resource_kind_lineage(definitions, definition);
        validate_nearest_resource_thumbnail_provider(
            definition.kind().as_str(),
            &lineage,
            actions,
        )?;
    }
    Ok(())
}

fn validate_nearest_resource_thumbnail_provider(
    kind: &str,
    lineage: &[&str],
    actions: &[ResourceActionDefinition],
) -> Result<(), CoreError> {
    let mut nearest = None;
    let mut providers = Vec::new();
    for action in actions.iter().filter(|action| {
        action
            .provides()
            .is_some_and(|capability| capability.as_str() == THUMBNAIL_CAPABILITY)
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
            "resource kind `{kind}` has multiple nearest `{THUMBNAIL_CAPABILITY}` providers: {}",
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
