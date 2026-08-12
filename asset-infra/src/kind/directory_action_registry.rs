use asset_core::{
    CoreError,
    domain::{ActionAccess, DefinitionOrigin, DirectoryActionDefinition},
    port::{DirectoryActionRegistry, DirectoryKindRegistry},
};
use asset_plugin_api::manifest::DirectoryActionCapability;

use super::normalization::directory_action_definition;
use super::validation::ensure_unique_scoped_action;
use crate::builtin_catalog::THUMBNAIL_CAPABILITY;

const DIRECTORY_THUMBNAIL_LOCATION: &str = "directory_thumbnail";

#[derive(Debug, Clone, Default)]
pub struct DefaultDirectoryActionRegistry {
    pub(super) actions: Vec<DirectoryActionDefinition>,
}

impl DirectoryActionRegistry for DefaultDirectoryActionRegistry {
    fn actions(&self) -> &[DirectoryActionDefinition] {
        &self.actions
    }
}

pub(super) fn push_directory_action(
    actions: &mut Vec<DirectoryActionDefinition>,
    capability: &DirectoryActionCapability,
    origin: DefinitionOrigin,
) -> Result<(), CoreError> {
    let source = origin.to_string();
    let action = directory_action_definition(capability, origin);
    push_directory_action_definition(actions, action, &source)
}

pub(super) fn push_directory_action_definition(
    actions: &mut Vec<DirectoryActionDefinition>,
    action: DirectoryActionDefinition,
    source: &str,
) -> Result<(), CoreError> {
    ensure_unique_scoped_action(
        "directory",
        action.id().as_str(),
        action.kinds(),
        source,
        actions
            .iter()
            .map(|existing| (existing.id().as_str(), existing.kinds())),
    )?;
    actions.push(action);
    Ok(())
}

pub(super) fn validate_directory_action_capabilities(
    kinds: &dyn DirectoryKindRegistry,
    actions: &[DirectoryActionDefinition],
) -> Result<(), CoreError> {
    for action in actions {
        if let Some(capability) = action
            .provides()
            .filter(|capability| capability.as_str() != THUMBNAIL_CAPABILITY)
        {
            return Err(CoreError::configuration(format!(
                "directory action `{}` provides unsupported capability `{capability}`",
                action.id(),
            )));
        }
        let in_thumbnail_slot = action
            .ui()
            .locations
            .iter()
            .any(|location| location == DIRECTORY_THUMBNAIL_LOCATION);
        let provides_thumbnail = action
            .provides()
            .is_some_and(|capability| capability.as_str() == THUMBNAIL_CAPABILITY);
        if in_thumbnail_slot != provides_thumbnail {
            return Err(CoreError::configuration(format!(
                "directory action `{}` must pair `{DIRECTORY_THUMBNAIL_LOCATION}` with capability `{THUMBNAIL_CAPABILITY}`",
                action.id()
            )));
        }
        if provides_thumbnail
            && (action.access() != ActionAccess::Read
                || !action.output().views.iter().any(|view| view == "media"))
        {
            return Err(CoreError::configuration(format!(
                "directory thumbnail provider `{}` must be read-only and support the `media` view",
                action.id()
            )));
        }
    }
    for definition in kinds.definitions() {
        validate_nearest_directory_thumbnail_provider(
            definition.kind().as_str(),
            &kinds
                .lineage(definition.kind())
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>(),
            actions,
        )?;
    }
    Ok(())
}

fn validate_nearest_directory_thumbnail_provider(
    kind: &str,
    lineage: &[&str],
    actions: &[DirectoryActionDefinition],
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
            "directory kind `{kind}` has multiple nearest `{THUMBNAIL_CAPABILITY}` providers: {}",
            providers.join(", ")
        )));
    }
    Ok(())
}
