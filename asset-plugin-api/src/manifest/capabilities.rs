use crate::PluginRuntime;
use crate::{
    ActionAccess, ActionDefinitionUi, ActionExecutorKind, ActionOutputContract,
    DirectoryActionAppliesTo, DirectoryActionDefinition, DirectoryActionRequirements,
    ResourceActionAccess, ResourceActionAppliesTo, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionExecutorKind, ResourceActionOutputContract,
    ResourceActionRequirements, ResourceActionUi, ResourceContentMatcher,
};
use serde::{Deserialize, Serialize};

/// Capabilities contributed by a plugin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginCapabilities {
    pub kinds: Vec<ResourceKindCapability>,
    pub directory_kinds: Vec<DirectoryKindCapability>,
    pub actions: Vec<ResourceActionCapability>,
    pub directory_actions: Vec<DirectoryActionCapability>,
}

/// Directory kind contributed by a plugin manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectoryKindCapability {
    pub kind: String,
    pub parent: Option<String>,
    pub label: Option<String>,
}

/// Resource kind contributed by a plugin manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceKindCapability {
    pub kind: String,
    pub parent: Option<String>,
    pub label: Option<String>,
    pub supports_content: bool,
    pub detect: ResourceContentMatcher,
}

impl Default for ResourceKindCapability {
    fn default() -> Self {
        Self {
            kind: String::new(),
            parent: None,
            label: None,
            supports_content: true,
            detect: ResourceContentMatcher::default(),
        }
    }
}

/// Resource action contributed by a plugin manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceActionCapability {
    pub id: String,
    pub label: String,
    pub handler: String,
    #[serde(default)]
    pub applies_to: ActionAppliesTo,
    #[serde(default)]
    pub access: ManifestActionAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<ActionRequirements>,
    pub views: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ActionUi>,
}

impl ResourceActionCapability {
    pub fn to_definition(&self, runtime: &PluginRuntime) -> ResourceActionDefinition {
        let mut definition = ResourceActionDefinition::new(self.id.clone(), self.label.clone())
            .with_handler(self.handler.clone())
            .with_executor(self.executor_kind(runtime))
            .with_access(self.access.to_resource_action_access())
            .with_applies_to(self.applies_to.to_definition())
            .with_output(ResourceActionOutputContract {
                view: self.views.clone(),
            });
        if let Some(requires) = &self.requires {
            definition = definition.with_requirements(requires.to_definition());
        }
        if let Some(ui) = &self.ui {
            definition = definition.with_ui(ResourceActionUi {
                group: ui.group.clone(),
                order: ui.order,
                locations: ui.locations.clone(),
            });
        }
        definition
    }

    pub fn executor_kind(&self, runtime: &PluginRuntime) -> ResourceActionExecutorKind {
        match runtime {
            PluginRuntime::Builtin => ResourceActionExecutorKind::Builtin,
            PluginRuntime::Extism { .. } => ResourceActionExecutorKind::Plugin,
        }
    }

    pub fn handler(&self) -> &str {
        self.handler.as_str()
    }
}

/// Directory action contributed by a plugin manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryActionCapability {
    pub id: String,
    pub label: String,
    pub handler: String,
    #[serde(default)]
    pub applies_to: DirectoryActionAppliesToCapability,
    #[serde(default)]
    pub access: ManifestActionAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<DirectoryActionRequirementsCapability>,
    pub views: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ActionUi>,
}

impl DirectoryActionCapability {
    pub fn to_definition(&self, runtime: &PluginRuntime) -> DirectoryActionDefinition {
        let mut definition = DirectoryActionDefinition::new(self.id.clone(), self.label.clone())
            .with_handler(self.handler.clone())
            .with_executor(match runtime {
                PluginRuntime::Builtin => ActionExecutorKind::Builtin,
                PluginRuntime::Extism { .. } => ActionExecutorKind::Plugin,
            })
            .with_access(match self.access {
                ManifestActionAccess::Read => ActionAccess::ReadOnly,
                ManifestActionAccess::Write => ActionAccess::ReadWrite,
            })
            .with_applies_to(self.applies_to.to_definition())
            .with_output(ActionOutputContract {
                view: self.views.clone(),
            });
        if let Some(requires) = &self.requires {
            definition = definition.with_requirements(requires.to_definition());
        }
        if let Some(ui) = &self.ui {
            definition = definition.with_ui(ActionDefinitionUi {
                group: ui.group.clone(),
                order: ui.order,
                locations: ui.locations.clone(),
            });
        }
        definition
    }

    pub fn handler(&self) -> &str {
        &self.handler
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectoryActionAppliesToCapability {
    pub kinds: Vec<String>,
}

impl DirectoryActionAppliesToCapability {
    pub fn to_definition(&self) -> DirectoryActionAppliesTo {
        DirectoryActionAppliesTo::new().with_kinds(self.kinds.clone())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectoryActionRequirementsCapability {
    pub children: bool,
    pub resources: bool,
}

impl DirectoryActionRequirementsCapability {
    pub fn to_definition(&self) -> DirectoryActionRequirements {
        DirectoryActionRequirements {
            children: self.children,
            resources: self.resources,
        }
    }
}

/// Manifest-level action access declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ManifestActionAccess {
    #[default]
    Read,
    Write,
}

impl ManifestActionAccess {
    pub fn to_resource_action_access(self) -> ResourceActionAccess {
        match self {
            Self::Read => ResourceActionAccess::ReadOnly,
            Self::Write => ResourceActionAccess::ReadWrite,
        }
    }
}

/// Resource/action matching declaration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionAppliesTo {
    pub kinds: Vec<String>,
    pub media_types: Vec<String>,
    pub extensions: Vec<String>,
}

impl ActionAppliesTo {
    pub fn to_definition(&self) -> ResourceActionAppliesTo {
        ResourceActionAppliesTo::new()
            .with_kinds(self.kinds.clone())
            .with_mime_types(self.media_types.clone())
            .with_extensions(self.extensions.clone())
    }
}

/// Optional object content a handler needs in addition to the resource snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRequirements {
    #[serde(default)]
    pub content: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_delivery: Option<ContentDelivery>,
}

impl ActionRequirements {
    pub fn to_definition(&self) -> ResourceActionRequirements {
        ResourceActionRequirements {
            content: self.content,
            content_delivery: self
                .content_delivery
                .map(ContentDelivery::to_resource_delivery)
                .unwrap_or(ResourceActionContentDelivery::Auto),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentDelivery {
    Inline,
    Reference,
}

impl ContentDelivery {
    pub fn to_resource_delivery(self) -> ResourceActionContentDelivery {
        match self {
            Self::Inline => ResourceActionContentDelivery::Inline,
            Self::Reference => ResourceActionContentDelivery::Reference,
        }
    }
}

#[cfg(test)]
mod tests;

/// Optional UI placement hints for host applications.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionUi {
    pub group: Option<String>,
    pub order: Option<i32>,
    pub locations: Vec<String>,
}
