use serde::{Deserialize, Serialize};

/// Resource action identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceAction(String);

impl ResourceAction {
    pub const DOWNLOAD_CONTENT: &'static str = "download_content";
    pub const READ: &'static str = "read";
    pub const VIEW_INLINE: &'static str = "view_inline";
    pub const PREVIEW: &'static str = "preview";
    pub const THUMBNAIL: &'static str = "thumbnail";

    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_string())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for ResourceAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for ResourceAction {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ResourceAction {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ResourceAction {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Resource action access boundary used by host execution requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResourceActionAccess {
    #[default]
    ReadOnly,
    ReadWrite,
}

/// Resource kind action declaration after manifest capabilities are resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceActionDefinition {
    id: ResourceAction,
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    handler: Option<String>,
    #[serde(default)]
    access: ResourceActionAccess,
    #[serde(default, skip_serializing_if = "ResourceActionWhen::is_empty")]
    when: ResourceActionWhen,
}

/// Content matching rules for an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceActionWhen {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mime_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extensions: Vec<String>,
}

impl ResourceActionDefinition {
    pub fn new(id: impl Into<ResourceAction>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            handler: None,
            access: ResourceActionAccess::ReadOnly,
            when: ResourceActionWhen::default(),
        }
    }

    pub fn with_handler(mut self, handler: impl Into<String>) -> Self {
        self.handler = Some(handler.into());
        self
    }

    pub fn with_access(mut self, access: ResourceActionAccess) -> Self {
        self.access = access;
        self
    }

    pub fn with_when(mut self, when: ResourceActionWhen) -> Self {
        self.when = when;
        self
    }

    pub fn id(&self) -> &ResourceAction {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn handler(&self) -> Option<&str> {
        self.handler.as_deref()
    }

    pub fn access(&self) -> ResourceActionAccess {
        self.access
    }

    pub fn when(&self) -> &ResourceActionWhen {
        &self.when
    }

    pub fn matches_content(&self, mime_type: Option<&str>, storage_key: Option<&str>) -> bool {
        self.when.matches(mime_type, storage_key)
    }
}

impl ResourceActionWhen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mime_types(
        mut self,
        mime_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.mime_types = mime_types
            .into_iter()
            .map(|value| value.into().trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect();
        self
    }

    pub fn with_extensions(
        mut self,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.extensions = extensions
            .into_iter()
            .map(normalize_extension)
            .filter(|value| !value.is_empty())
            .collect();
        self
    }

    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    pub fn is_empty(&self) -> bool {
        self.mime_types.is_empty() && self.extensions.is_empty()
    }

    pub fn matches(&self, mime_type: Option<&str>, storage_key: Option<&str>) -> bool {
        if self.is_empty() {
            return true;
        }

        let mime_type = mime_type.map(|value| value.to_ascii_lowercase());
        if let Some(mime_type) = mime_type.as_deref() {
            if self
                .mime_types
                .iter()
                .any(|expected| mime_matches(expected, mime_type))
            {
                return true;
            }
        }

        let storage_key = storage_key.map(|value| value.to_ascii_lowercase());
        if let Some(storage_key) = storage_key.as_deref() {
            if self
                .extensions
                .iter()
                .any(|extension| storage_key.ends_with(extension))
            {
                return true;
            }
        }

        false
    }
}

fn normalize_extension(value: impl Into<String>) -> String {
    let value = value.into().trim().to_ascii_lowercase();
    if value.is_empty() {
        return value;
    }
    if value.starts_with('.') {
        value
    } else {
        format!(".{value}")
    }
}

fn mime_matches(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }
    expected
        .strip_suffix("/*")
        .is_some_and(|prefix| actual.starts_with(&format!("{prefix}/")))
}
