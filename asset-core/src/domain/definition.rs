use crate::domain::{DirectoryKind, ResourceContentMatcher, ResourceKind};
use thiserror::Error;

const MAX_DEFINITION_ORIGIN_ID_LEN: usize = 256;

/// Canonical identity of the built-in module or plugin that owns a definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefinitionOriginId(String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DefinitionOriginIdError {
    #[error("definition origin id cannot be blank")]
    Blank,
    #[error("definition origin id must not have leading or trailing whitespace")]
    NonCanonical,
    #[error("definition origin id must not exceed {max} characters")]
    TooLong { max: usize },
    #[error("definition origin id contains invalid characters: `{value}`")]
    InvalidFormat { value: String },
}

impl DefinitionOriginId {
    pub fn new(value: impl Into<String>) -> Result<Self, DefinitionOriginIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(DefinitionOriginIdError::Blank);
        }
        if value.trim() != value {
            return Err(DefinitionOriginIdError::NonCanonical);
        }
        if value.chars().count() > MAX_DEFINITION_ORIGIN_ID_LEN {
            return Err(DefinitionOriginIdError::TooLong {
                max: MAX_DEFINITION_ORIGIN_ID_LEN,
            });
        }
        if !value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.chars().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || matches!(character, '-' | '_')
                })
        }) {
            return Err(DefinitionOriginIdError::InvalidFormat { value });
        }
        Ok(Self(value))
    }

    pub fn from_static(value: &'static str) -> Self {
        Self::new(value).expect("static definition origin id must be canonical")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable origin of a Host-normalized kind or action declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DefinitionOrigin {
    Builtin { id: DefinitionOriginId },
    Plugin { id: DefinitionOriginId },
}

impl DefinitionOrigin {
    pub fn builtin(id: impl Into<String>) -> Result<Self, DefinitionOriginIdError> {
        Ok(Self::Builtin {
            id: DefinitionOriginId::new(id)?,
        })
    }

    pub fn plugin(id: impl Into<String>) -> Result<Self, DefinitionOriginIdError> {
        Ok(Self::Plugin {
            id: DefinitionOriginId::new(id)?,
        })
    }

    pub fn builtin_static(id: &'static str) -> Self {
        Self::Builtin {
            id: DefinitionOriginId::from_static(id),
        }
    }

    pub fn plugin_static(id: &'static str) -> Self {
        Self::Plugin {
            id: DefinitionOriginId::from_static(id),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Builtin { .. } => "builtin",
            Self::Plugin { .. } => "plugin",
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Builtin { id } | Self::Plugin { id } => id.as_str(),
        }
    }
}

impl std::fmt::Display for DefinitionOrigin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.kind(), self.id())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceKindDefinition {
    kind: ResourceKind,
    parent: Option<ResourceKind>,
    label: String,
    supports_content: bool,
    detect: ResourceContentMatcher,
    origin: DefinitionOrigin,
}

impl ResourceKindDefinition {
    pub fn new(
        kind: ResourceKind,
        label: impl Into<String>,
        supports_content: bool,
        origin: DefinitionOrigin,
    ) -> Self {
        Self {
            kind,
            parent: None,
            label: label.into(),
            supports_content,
            detect: ResourceContentMatcher::default(),
            origin,
        }
    }

    pub fn with_parent(mut self, parent: Option<ResourceKind>) -> Self {
        self.parent = parent;
        self
    }

    pub fn with_detect(mut self, detect: ResourceContentMatcher) -> Self {
        self.detect = detect;
        self
    }

    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    pub fn parent(&self) -> Option<&ResourceKind> {
        self.parent.as_ref()
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn supports_content(&self) -> bool {
        self.supports_content
    }

    pub fn detect(&self) -> &ResourceContentMatcher {
        &self.detect
    }

    pub fn origin(&self) -> &DefinitionOrigin {
        &self.origin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryKindDefinition {
    kind: DirectoryKind,
    parent: Option<DirectoryKind>,
    default_child_kind: Option<DirectoryKind>,
    allowed_parent_kinds: Vec<DirectoryKind>,
    label: String,
    origin: DefinitionOrigin,
}

impl DirectoryKindDefinition {
    pub fn new(kind: DirectoryKind, label: impl Into<String>, origin: DefinitionOrigin) -> Self {
        Self {
            kind,
            parent: None,
            default_child_kind: None,
            allowed_parent_kinds: Vec::new(),
            label: label.into(),
            origin,
        }
    }

    pub fn with_parent(mut self, parent: Option<DirectoryKind>) -> Self {
        self.parent = parent;
        self
    }

    pub fn with_allowed_parent_kinds(
        mut self,
        kinds: impl IntoIterator<Item = DirectoryKind>,
    ) -> Self {
        self.allowed_parent_kinds = kinds.into_iter().collect();
        self
    }

    pub fn with_default_child_kind(mut self, kind: Option<DirectoryKind>) -> Self {
        self.default_child_kind = kind;
        self
    }

    pub fn kind(&self) -> &DirectoryKind {
        &self.kind
    }

    pub fn parent(&self) -> Option<&DirectoryKind> {
        self.parent.as_ref()
    }

    pub fn allowed_parent_kinds(&self) -> &[DirectoryKind] {
        &self.allowed_parent_kinds
    }

    pub fn default_child_kind(&self) -> Option<&DirectoryKind> {
        self.default_child_kind.as_ref()
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn origin(&self) -> &DefinitionOrigin {
        &self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_origins_reject_non_canonical_owner_ids() {
        assert_eq!(
            DefinitionOrigin::plugin("example.plugin")
                .unwrap()
                .to_string(),
            "plugin:example.plugin"
        );
        for value in ["", " Plugin", "EXAMPLE.Plugin", "example/plugin"] {
            assert!(
                DefinitionOrigin::plugin(value).is_err(),
                "`{value}` must be rejected"
            );
        }
    }
}
