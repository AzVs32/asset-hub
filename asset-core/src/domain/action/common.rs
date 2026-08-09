use crate::domain::DefinitionOrigin;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ActionIdError {
    #[error("{kind} cannot be blank")]
    Blank { kind: &'static str },
    #[error("{kind} must not have leading or trailing whitespace")]
    NonCanonical { kind: &'static str },
    #[error("{kind} contains invalid characters: `{value}`")]
    InvalidFormat { kind: &'static str, value: String },
}

/// Action identifier shared by resource and directory targets.
///
/// Action IDs are extensible and globally unique. External Manifest adapters preserve validated
/// `<plugin-id>.<verb>` identifiers when constructing this value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionId(String);

impl ActionId {
    pub fn new(value: impl Into<String>) -> Result<Self, ActionIdError> {
        let value = validate_id("action id", value.into(), &['.', '-', '_'])?;
        if !value.split('.').all(valid_action_segment) || !value.contains('.') {
            return Err(ActionIdError::InvalidFormat {
                kind: "action id",
                value,
            });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn from_static(value: &'static str) -> Self {
        Self::new(value).expect("static action id must be valid")
    }
}

fn valid_action_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
}

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for ActionId {
    type Error = ActionIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for ActionId {
    type Error = ActionIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for ActionId {
    type Err = ActionIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl AsRef<str> for ActionId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Semantic singleton capability implemented by one of several action providers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionCapabilityId(String);

impl ActionCapabilityId {
    pub fn new(value: impl Into<String>) -> Result<Self, ActionIdError> {
        validate_id("action capability id", value.into(), &['.', '-', '_']).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn from_static(value: &'static str) -> Self {
        Self::new(value).expect("static action capability id must be valid")
    }
}

impl std::fmt::Display for ActionCapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for ActionCapabilityId {
    type Error = ActionIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for ActionCapabilityId {
    type Error = ActionIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for ActionCapabilityId {
    type Err = ActionIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl AsRef<str> for ActionCapabilityId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

fn validate_id(
    kind: &'static str,
    value: String,
    punctuation: &[char],
) -> Result<String, ActionIdError> {
    if value.is_empty() {
        return Err(ActionIdError::Blank { kind });
    }
    if value.trim() != value {
        return Err(ActionIdError::NonCanonical { kind });
    }
    if !value.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || punctuation.contains(&character)
    }) {
        return Err(ActionIdError::InvalidFormat { kind, value });
    }
    Ok(value)
}

/// Resource and directory action access boundary used by host execution requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionAccess {
    #[default]
    Read,
    Write,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionOutputContract {
    pub views: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionUi {
    pub group: Option<String>,
    pub order: Option<i32>,
    pub locations: Vec<String>,
}

/// Target-independent action declaration fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDefinition {
    id: ActionId,
    origin: DefinitionOrigin,
    provides: Option<ActionCapabilityId>,
    label: String,
    description: Option<String>,
    access: ActionAccess,
    output: ActionOutputContract,
    ui: ActionUi,
}

impl ActionDefinition {
    pub fn new(id: ActionId, label: impl Into<String>, origin: DefinitionOrigin) -> Self {
        Self {
            id,
            origin,
            provides: None,
            label: label.into(),
            description: None,
            access: ActionAccess::Read,
            output: ActionOutputContract::default(),
            ui: ActionUi::default(),
        }
    }

    pub fn new_static(
        id: &'static str,
        label: impl Into<String>,
        origin: DefinitionOrigin,
    ) -> Self {
        Self::new(ActionId::from_static(id), label, origin)
    }

    pub fn with_provides(mut self, provides: Option<ActionCapabilityId>) -> Self {
        self.provides = provides;
        self
    }

    pub fn with_static_provides(mut self, provides: Option<&'static str>) -> Self {
        self.provides = provides.map(ActionCapabilityId::from_static);
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_access(mut self, access: ActionAccess) -> Self {
        self.access = access;
        self
    }

    pub fn with_output(mut self, output: ActionOutputContract) -> Self {
        self.output = output;
        self
    }

    pub fn with_ui(mut self, ui: ActionUi) -> Self {
        self.ui = ui;
        self
    }

    pub fn id(&self) -> &ActionId {
        &self.id
    }

    pub fn origin(&self) -> &DefinitionOrigin {
        &self.origin
    }

    pub fn provides(&self) -> Option<&ActionCapabilityId> {
        self.provides.as_ref()
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn access(&self) -> ActionAccess {
        self.access
    }

    pub fn output(&self) -> &ActionOutputContract {
        &self.output
    }

    pub fn ui(&self) -> &ActionUi {
        &self.ui
    }
}
