//! External Manifest declarations to Host/Core model conversion.

use asset_core::domain::{
    ActionAccess, ActionCapabilityId, ActionId, ActionOutputContract,
    ActionUi as ActionDefinitionUi, DefinitionOrigin, DirectoryActionAppliesTo,
    DirectoryActionDefinition, DirectoryActionRequirements, DirectoryResourceAccess,
    ResourceActionAppliesTo, ResourceActionContentDelivery, ResourceActionDefinition,
    ResourceActionRequirements, ResourceContentMatcher,
};
use asset_plugin_sdk::manifest::{
    ContentDelivery, DirectoryActionCapability, ManifestActionAccess, ResourceActionCapability,
};

pub(super) fn resource_action_definition(
    capability: &ResourceActionCapability,
    label: &str,
    origin: DefinitionOrigin,
) -> ResourceActionDefinition {
    let mut definition = ResourceActionDefinition::new(
        ActionId::new(capability.id.clone()).expect("validated manifest resource action id"),
        label,
        origin,
    )
    .with_description(capability.description.clone())
    .with_provides(validated_capability_id(capability.provides.as_deref()))
    .with_access(action_access(capability.access))
    .with_applies_to(resource_action_applies_to(capability))
    .with_output(ActionOutputContract {
        views: capability.output.views.clone(),
        effects: capability.output.effects.clone(),
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
            ..ActionDefinitionUi::default()
        });
    }
    if capability
        .output
        .effects
        .iter()
        .any(|effect| effect == "delete")
    {
        let ui = definition.ui().clone();
        definition = definition.with_ui(ActionDefinitionUi {
            group: ui.group,
            order: ui.order,
            locations: ui.locations,
            destructive: true,
            confirmation: Some("Delete {name}?".to_string()),
        });
    }
    definition
}

pub(super) fn directory_action_definition(
    capability: &DirectoryActionCapability,
    origin: DefinitionOrigin,
) -> DirectoryActionDefinition {
    let mut definition = DirectoryActionDefinition::new(
        ActionId::new(capability.id.clone()).expect("validated manifest directory action id"),
        capability.label.as_str(),
        origin,
    )
    .with_description(capability.description.clone())
    .with_provides(validated_capability_id(capability.provides.as_deref()))
    .with_access(action_access(capability.access))
    .with_applies_to(
        DirectoryActionAppliesTo::new().with_kinds(capability.applies_to.kinds.clone()),
    )
    .with_output(ActionOutputContract {
        views: capability.output.views.clone(),
        effects: capability.output.effects.clone(),
    });
    if let Some(requirements) = &capability.requires {
        definition = definition.with_requirements(DirectoryActionRequirements {
            children: requirements.children,
            resources: match requirements.resources {
                asset_plugin_sdk::manifest::DirectoryResourceAccess::None => {
                    DirectoryResourceAccess::None
                }
                asset_plugin_sdk::manifest::DirectoryResourceAccess::Metadata => {
                    DirectoryResourceAccess::Metadata
                }
                asset_plugin_sdk::manifest::DirectoryResourceAccess::Content => {
                    DirectoryResourceAccess::Content
                }
            },
        });
    }
    if let Some(ui) = &capability.ui {
        definition = definition.with_ui(ActionDefinitionUi {
            group: ui.group.clone(),
            order: ui.order,
            locations: ui.locations.clone(),
            ..ActionDefinitionUi::default()
        });
    }
    if capability
        .output
        .effects
        .iter()
        .any(|effect| effect == "delete")
    {
        let ui = definition.ui().clone();
        definition = definition.with_ui(ActionDefinitionUi {
            group: ui.group,
            order: ui.order,
            locations: ui.locations,
            destructive: true,
            confirmation: Some("Delete empty directory {name}?".to_string()),
        });
    }
    definition
}

pub(crate) fn resource_action_applies_to(
    capability: &ResourceActionCapability,
) -> ResourceActionAppliesTo {
    ResourceActionAppliesTo::new()
        .with_kinds(capability.applies_to.kinds.clone())
        .with_mime_types(capability.applies_to.mime_types.clone())
        .with_extensions(capability.applies_to.extensions.clone())
}

pub(super) fn content_matcher(
    matcher: &asset_plugin_sdk::manifest::ResourceContentMatcher,
) -> ResourceContentMatcher {
    ResourceContentMatcher::new()
        .with_mime_types(matcher.mime_types().iter().cloned())
        .with_extensions(matcher.extensions().iter().cloned())
}

fn action_access(access: ManifestActionAccess) -> ActionAccess {
    match access {
        ManifestActionAccess::Read => ActionAccess::Read,
        ManifestActionAccess::Write => ActionAccess::Write,
    }
}

fn validated_capability_id(value: Option<&str>) -> Option<ActionCapabilityId> {
    value.map(|value| {
        ActionCapabilityId::new(value).expect("validated manifest action capability id")
    })
}

fn content_delivery(delivery: ContentDelivery) -> ResourceActionContentDelivery {
    match delivery {
        ContentDelivery::Inline => ResourceActionContentDelivery::Inline,
        ContentDelivery::Reference => ResourceActionContentDelivery::Reference,
    }
}
