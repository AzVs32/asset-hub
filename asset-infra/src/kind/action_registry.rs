use asset_core::CoreError;
use asset_core::port::{ResourceActionRegistry, ResourceKindDefinition};
use asset_plugin_api::{
    PluginRuntime, ResourceActionCapability, ResourceActionDefinition, ResourceContentMatcher,
};

/// 默认资源动作注册表。
#[derive(Debug, Clone)]
pub(crate) struct DefaultResourceActionRegistry {
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
