use super::common::{
    ActionAccess, ActionCapabilityId, ActionDefinition, ActionId, ActionOutputContract, ActionUi,
};
use super::matcher::normalize_kinds;

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
    pub fn matches(&self, kind: &str) -> bool {
        self.kinds.is_empty()
            || self
                .kinds
                .iter()
                .any(|expected| expected.eq_ignore_ascii_case(kind))
    }
}

/// Directory data a handler expects to query through paginated Host APIs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectoryActionRequirements {
    pub children: bool,
    pub resources: bool,
}

/// Directory action declaration after manifest capabilities are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryActionDefinition {
    action: ActionDefinition,
    applies_to: DirectoryActionAppliesTo,
    requires: DirectoryActionRequirements,
}

impl DirectoryActionDefinition {
    pub fn new(id: ActionId, label: impl Into<String>) -> Self {
        Self {
            action: ActionDefinition::new(id, label),
            applies_to: DirectoryActionAppliesTo::default(),
            requires: DirectoryActionRequirements::default(),
        }
    }
    pub fn new_static(id: &'static str, label: impl Into<String>) -> Self {
        Self::new(ActionId::from_static(id), label)
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
    pub fn matches_directory(&self, kind: &str) -> bool {
        self.applies_to.matches(kind)
    }
}

pub type DirectoryAction = ActionId;
pub type DirectoryActionAccess = ActionAccess;
pub type DirectoryActionOutputContract = ActionOutputContract;
pub type DirectoryActionUi = ActionUi;
