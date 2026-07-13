use crate::{
    ResourceActionAccess, ResourceActionAppliesTo, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionExecutorKind, ResourceActionOutputContract,
    ResourceActionRequirements, ResourceActionUi, ResourceContentMatcher,
};
use serde::{Deserialize, Serialize};

/// Capabilities contributed by a plugin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginCapabilities {
    pub resource_kinds: Vec<ResourceKindCapability>,
    pub resource_actions: Vec<ResourceActionCapability>,
}

/// Resource kind contributed by a plugin manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
pub struct ResourceActionCapability {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor: Option<ActionExecutor>,
    #[serde(default)]
    pub applies_to: ActionAppliesTo,
    #[serde(default)]
    pub access: ManifestActionAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<ActionRequirements>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<ActionOutputContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ActionUi>,
}

impl ResourceActionCapability {
    pub fn to_definition(&self) -> ResourceActionDefinition {
        let mut definition = ResourceActionDefinition::new(self.id.clone(), self.label.clone())
            .with_description(self.description.clone())
            .with_access(self.access.to_resource_action_access())
            .with_applies_to(self.applies_to.to_definition());
        if let Some(requires) = &self.requires {
            definition = definition.with_requirements(requires.to_definition());
        }
        if let Some(output) = &self.output {
            definition = definition.with_output(ResourceActionOutputContract {
                view: output.view.clone(),
            });
        }
        if let Some(ui) = &self.ui {
            definition = definition.with_ui(ResourceActionUi {
                group: ui.group.clone(),
                order: ui.order,
                locations: ui.locations.clone(),
            });
        }
        if let Some(handler) = self.handler() {
            definition = definition.with_handler(handler);
        }
        definition = definition.with_executor(self.executor_kind());
        definition
    }

    pub fn executor_kind(&self) -> ResourceActionExecutorKind {
        match &self.executor {
            Some(ActionExecutor::Plugin { .. }) => ResourceActionExecutorKind::Plugin,
            _ => ResourceActionExecutorKind::Builtin,
        }
    }

    pub fn handler(&self) -> Option<&str> {
        match &self.executor {
            Some(ActionExecutor::Builtin { handler })
            | Some(ActionExecutor::Plugin { handler }) => Some(handler.as_str()),
            _ => None,
        }
    }

    pub fn plugin_handler(&self) -> Option<&str> {
        match &self.executor {
            Some(ActionExecutor::Plugin { handler }) => Some(handler.as_str()),
            _ => None,
        }
    }
}

/// Action executor declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionExecutor {
    Builtin { handler: String },
    Plugin { handler: String },
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
#[serde(default)]
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

/// Data a handler needs to execute an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRequirements {
    #[serde(default)]
    pub resource: bool,
    #[serde(default)]
    pub metadata: bool,
    #[serde(default)]
    pub content: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_delivery: Option<ContentDelivery>,
}

impl ActionRequirements {
    pub fn to_definition(&self) -> ResourceActionRequirements {
        ResourceActionRequirements {
            resource: self.resource,
            metadata: self.metadata,
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
    Url,
}

impl ContentDelivery {
    pub fn to_resource_delivery(self) -> ResourceActionContentDelivery {
        match self {
            Self::Inline => ResourceActionContentDelivery::Inline,
            Self::Url => ResourceActionContentDelivery::Url,
        }
    }
}

/// Declared output view families for an action.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionOutputContract {
    pub view: Vec<String>,
}

/// Optional UI placement hints for host applications.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionUi {
    pub group: Option<String>,
    pub order: Option<i32>,
    pub locations: Vec<String>,
}
