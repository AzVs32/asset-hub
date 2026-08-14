use super::common::{
    ActionAccess, ActionCapabilityId, ActionDefinition, ActionId, ActionOutputContract, ActionUi,
};
use super::matcher::normalize_kinds;
use crate::domain::DefinitionOrigin;

/// Directory/action matching rules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectoryActionAppliesTo {
    kinds: Vec<String>,
}

impl DirectoryActionAppliesTo {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_kinds(mut self, kinds: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.kinds = normalize_kinds(kinds);
        self
    }
    pub fn kinds(&self) -> &[String] {
        &self.kinds
    }
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
    /// Match one exact kind after an adapter has expanded inherited applicability.
    ///
    /// Application-level inheritance is resolved from the Directory kind lineage by the action
    /// registry; this predicate deliberately does not walk that lineage.
    pub fn matches_exact_kind(&self, kind: &str) -> bool {
        self.kinds.is_empty() || self.kinds.iter().any(|expected| expected == kind)
    }
}

/// Resource data exposed to a Directory Action through paginated Host APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirectoryResourceAccess {
    #[default]
    None,
    Metadata,
    Content,
}

impl DirectoryResourceAccess {
    pub fn includes_metadata(self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn includes_content(self) -> bool {
        matches!(self, Self::Content)
    }
}

/// Directory data a handler expects to query through call-scoped Host APIs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectoryActionRequirements {
    pub children: bool,
    pub resources: DirectoryResourceAccess,
}

/// Directory action declaration after manifest capabilities are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryActionDefinition {
    action: ActionDefinition,
    applies_to: DirectoryActionAppliesTo,
    requires: DirectoryActionRequirements,
}

/// Directory-scoped action identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirectoryActionId(ActionId);

impl DirectoryActionId {
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

impl std::fmt::Display for DirectoryActionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for DirectoryActionId {
    type Err = super::common::ActionIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for DirectoryActionId {
    type Error = super::common::ActionIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl DirectoryActionDefinition {
    pub fn new(id: ActionId, label: impl Into<String>, origin: DefinitionOrigin) -> Self {
        Self {
            action: ActionDefinition::new(id, label, origin),
            applies_to: DirectoryActionAppliesTo::default(),
            requires: DirectoryActionRequirements::default(),
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
    pub fn with_applies_to(mut self, applies_to: DirectoryActionAppliesTo) -> Self {
        self.applies_to = applies_to;
        self
    }
    pub fn with_kinds(mut self, kinds: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.applies_to = self.applies_to.with_kinds(kinds);
        self
    }
    pub fn with_requirements(mut self, requirements: DirectoryActionRequirements) -> Self {
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
    pub fn requirements(&self) -> &DirectoryActionRequirements {
        &self.requires
    }
    pub fn applies_to(&self) -> &DirectoryActionAppliesTo {
        &self.applies_to
    }
    pub fn kinds(&self) -> &[String] {
        self.applies_to.kinds()
    }
    pub fn matches_exact_kind(&self, kind: &str) -> bool {
        self.applies_to.matches_exact_kind(kind)
    }
}
