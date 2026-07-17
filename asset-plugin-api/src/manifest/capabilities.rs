use crate::PluginRuntime;
use crate::{
    ResourceActionAccess, ResourceActionAppliesTo, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionExecutorKind, ResourceActionOutputContract,
    ResourceActionRequirements, ResourceActionUi, ResourceContentMatcher,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Capabilities contributed by a plugin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginCapabilities {
    #[serde(rename = "kinds", alias = "resource_kinds")]
    pub resource_kinds: Vec<ResourceKindCapability>,
    #[serde(rename = "actions", alias = "resource_actions")]
    pub resource_actions: Vec<ResourceActionCapability>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ResourceKindMetadataCapability>,
}

impl Default for ResourceKindCapability {
    fn default() -> Self {
        Self {
            kind: String::new(),
            parent: None,
            label: None,
            supports_content: true,
            detect: ResourceContentMatcher::default(),
            metadata: None,
        }
    }
}

/// Versioned JSON Schema describing intrinsic metadata owned by a resource kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceKindMetadataCapability {
    pub schema_version: u32,
    pub schema: Value,
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
