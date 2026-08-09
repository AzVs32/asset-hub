use super::common::{
    ActionAccess, ActionCapabilityId, ActionDefinition, ActionId, ActionOutputContract, ActionUi,
};
use super::matcher::{ResourceContentMatcher, normalize_kinds};
use crate::domain::DefinitionOrigin;

/// How the host should deliver object content to an action handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResourceActionContentDelivery {
    #[default]
    Auto,
    Inline,
    Reference,
}

/// Optional object content required in addition to the resource snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceActionRequirements {
    pub content: bool,
    pub content_delivery: ResourceActionContentDelivery,
}

impl Default for ResourceActionRequirements {
    fn default() -> Self {
        Self {
            content: false,
            content_delivery: ResourceActionContentDelivery::Auto,
        }
    }
}

/// Resource/action matching rules used by action availability.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceActionAppliesTo {
    kinds: Vec<String>,
    content: ResourceContentMatcher,
}

impl ResourceActionAppliesTo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_kinds(mut self, kinds: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.kinds = normalize_kinds(kinds);
        self
    }

    pub fn with_mime_types(
        mut self,
        mime_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.content = self.content.with_mime_types(mime_types);
        self
    }

    pub fn with_extensions(
        mut self,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.content = self.content.with_extensions(extensions);
        self
    }

    pub fn with_content_matcher(mut self, content: ResourceContentMatcher) -> Self {
        self.content = content;
        self
    }

    pub fn kinds(&self) -> &[String] {
        &self.kinds
    }

    pub fn content(&self) -> &ResourceContentMatcher {
        &self.content
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.content.is_empty()
    }

    pub fn matches_content(&self, mime_type: Option<&str>, storage_key: Option<&str>) -> bool {
        self.content.matches_content(mime_type, storage_key)
    }

    pub fn matches_resource(
        &self,
        kind: &str,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> bool {
        if !self.kinds.is_empty() && !self.kinds.iter().any(|expected| expected == kind) {
            return false;
        }

        self.content.matches_content(mime_type, storage_key)
    }
}

/// Resource action declaration after manifest capabilities are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceActionDefinition {
    action: ActionDefinition,
    applies_to: ResourceActionAppliesTo,
    requires: ResourceActionRequirements,
}

/// Resource-scoped action identity. Directory action IDs use a distinct type even when their
/// serialized text happens to be equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceActionId(ActionId);

impl ResourceActionId {
    pub fn new(value: impl Into<String>) -> Result<Self, super::common::ActionIdError> {
        ActionId::new(value).map(Self)
    }

    pub fn from_static(value: &'static str) -> Self {
        Self(ActionId::from_static(value))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for ResourceActionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for ResourceActionId {
    type Err = super::common::ActionIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for ResourceActionId {
    type Error = super::common::ActionIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl ResourceActionDefinition {
    pub fn new(id: ActionId, label: impl Into<String>, origin: DefinitionOrigin) -> Self {
        Self {
            action: ActionDefinition::new(id, label, origin),
            applies_to: ResourceActionAppliesTo::default(),
            requires: ResourceActionRequirements::default(),
        }
    }

    pub fn new_static(id: &'static str, label: impl Into<String>) -> Self {
        let origin = id.rsplit_once('.').map_or(id, |(namespace, _)| namespace);
        Self::new(
            ActionId::from_static(id),
            label,
            DefinitionOrigin::builtin_static(origin),
        )
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.action = self.action.with_description(description);
        self
    }
    pub fn with_provides(mut self, provides: Option<ActionCapabilityId>) -> Self {
        self.action = self.action.with_provides(provides);
        self
    }
    pub fn with_static_provides(mut self, provides: Option<&'static str>) -> Self {
        self.action = self.action.with_static_provides(provides);
        self
    }
    pub fn with_access(mut self, access: ActionAccess) -> Self {
        self.action = self.action.with_access(access);
        self
    }
    pub fn with_output(mut self, output: ActionOutputContract) -> Self {
        self.action = self.action.with_output(output);
        self
    }
    pub fn with_ui(mut self, ui: ActionUi) -> Self {
        self.action = self.action.with_ui(ui);
        self
    }
    pub fn with_applies_to(mut self, applies_to: ResourceActionAppliesTo) -> Self {
        self.applies_to = applies_to;
        self
    }
    pub fn with_kinds(mut self, kinds: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.applies_to.kinds = normalize_kinds(kinds);
        self
    }
    pub fn with_content_matcher(mut self, content: ResourceContentMatcher) -> Self {
        self.applies_to.content = content;
        self
    }
    pub fn with_requirements(mut self, requirements: ResourceActionRequirements) -> Self {
        self.requires = requirements;
        self
    }

    pub fn common(&self) -> &ActionDefinition {
        &self.action
    }
    pub fn id(&self) -> &ActionId {
        self.action.id()
    }
    pub fn origin(&self) -> &DefinitionOrigin {
        self.action.origin()
    }
    pub fn provides(&self) -> Option<&ActionCapabilityId> {
        self.action.provides()
    }
    pub fn label(&self) -> &str {
        self.action.label()
    }
    pub fn description(&self) -> Option<&str> {
        self.action.description()
    }
    pub fn access(&self) -> ActionAccess {
        self.action.access()
    }
    pub fn output(&self) -> &ActionOutputContract {
        self.action.output()
    }
    pub fn ui(&self) -> &ActionUi {
        self.action.ui()
    }
    pub fn requirements(&self) -> &ResourceActionRequirements {
        &self.requires
    }
    pub fn applies_to(&self) -> &ResourceActionAppliesTo {
        &self.applies_to
    }
    pub fn kinds(&self) -> &[String] {
        self.applies_to.kinds()
    }
    pub fn content_matcher(&self) -> &ResourceContentMatcher {
        self.applies_to.content()
    }
    pub fn matches_content(&self, mime_type: Option<&str>, storage_key: Option<&str>) -> bool {
        self.applies_to.matches_content(mime_type, storage_key)
    }
    pub fn matches_resource(
        &self,
        kind: &str,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> bool {
        self.applies_to
            .matches_resource(kind, mime_type, storage_key)
    }
}
