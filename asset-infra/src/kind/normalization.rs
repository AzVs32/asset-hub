//! External Manifest declarations to Host/Core model conversion.

use asset_core::domain::{
    ActionAccess, ActionOutputContract, ActionUi as ActionDefinitionUi, DirectoryActionAppliesTo,
    DirectoryActionDefinition, DirectoryActionRequirements, ResourceActionAppliesTo,
    ResourceActionContentDelivery, ResourceActionDefinition, ResourceActionRequirements,
    ResourceContentMatcher,
};
use asset_plugin_api::manifest::{
    ContentDelivery, DirectoryActionCapability, ManifestActionAccess, ResourceActionCapability,
};

pub(super) fn resource_action_definition(
    capability: &ResourceActionCapability,
) -> ResourceActionDefinition {
    let mut definition =
        ResourceActionDefinition::new(capability.id.as_str(), capability.label.as_str())
            .with_provides(capability.provides.clone())
            .with_access(action_access(capability.access))
            .with_applies_to(resource_action_applies_to(capability))
            .with_output(ActionOutputContract {
                view: capability.views.clone(),
            });
    if let Some(requirements) = &capability.requires {
        definition = definition.with_requirements(ResourceActionRequirements {
            content: requirements.content,
            content_delivery: requirements
                .content_delivery
                .map(content_delivery)
                .unwrap_or_default(),
        });
    }
    if let Some(ui) = &capability.ui {
        definition = definition.with_ui(ActionDefinitionUi {
            group: ui.group.clone(),
            order: ui.order,
            locations: ui.locations.clone(),
        });
    }
    definition
}

pub(super) fn directory_action_definition(
    capability: &DirectoryActionCapability,
) -> DirectoryActionDefinition {
    let mut definition =
        DirectoryActionDefinition::new(capability.id.as_str(), capability.label.as_str())
            .with_provides(capability.provides.clone())
            .with_access(action_access(capability.access))
            .with_applies_to(
                DirectoryActionAppliesTo::new().with_kinds(capability.applies_to.kinds.clone()),
            )
            .with_output(ActionOutputContract {
                view: capability.views.clone(),
            });
    if let Some(requirements) = &capability.requires {
        definition = definition.with_requirements(DirectoryActionRequirements {
            children: requirements.children,
            resources: requirements.resources,
        });
    }
    if let Some(ui) = &capability.ui {
        definition = definition.with_ui(ActionDefinitionUi {
            group: ui.group.clone(),
            order: ui.order,
            locations: ui.locations.clone(),
        });
    }
    definition
}

pub(crate) fn resource_action_applies_to(
    capability: &ResourceActionCapability,
) -> ResourceActionAppliesTo {
    ResourceActionAppliesTo::new()
        .with_kinds(capability.applies_to.kinds.clone())
        .with_mime_types(capability.applies_to.media_types.clone())
        .with_extensions(capability.applies_to.extensions.clone())
}

pub(super) fn content_matcher(
    matcher: &asset_plugin_api::manifest::ResourceContentMatcher,
) -> ResourceContentMatcher {
    ResourceContentMatcher::new()
        .with_mime_types(matcher.mime_types().iter().cloned())
        .with_extensions(matcher.extensions().iter().cloned())
}

fn action_access(access: ManifestActionAccess) -> ActionAccess {
    match access {
        ManifestActionAccess::Read => ActionAccess::ReadOnly,
        ManifestActionAccess::Write => ActionAccess::ReadWrite,
    }
}

fn content_delivery(delivery: ContentDelivery) -> ResourceActionContentDelivery {
    match delivery {
        ContentDelivery::Inline => ResourceActionContentDelivery::Inline,
        ContentDelivery::Reference => ResourceActionContentDelivery::Reference,
    }
}
