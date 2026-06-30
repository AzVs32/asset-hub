//! Shared host/plugin API types.
//!
//! This crate is the only JSON contract shared by the host and plugin crates.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// Resource action access boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResourceActionAccess {
    #[default]
    ReadOnly,
    ReadWrite,
}

/// Resource kind action declaration.
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

/// Action request passed from host to a plugin handler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginActionRequest {
    pub action: String,
    pub access: ResourceActionAccess,
    #[serde(default)]
    pub input: Value,
    pub resource: PluginResource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginContentBytes>,
}

/// Resource snapshot exposed to plugin handlers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginResource {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginResourceContent>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

/// Resource content reference exposed to plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginResourceContent {
    pub key: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub checksum: Vec<PluginChecksum>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginChecksum {
    pub kind: String,
    pub value: String,
}

/// Inline object content supplied to a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginContentBytes {
    pub encoding: PluginContentEncoding,
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginContentEncoding {
    Base64,
}

/// Standard action output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginActionOutput {
    #[serde(flatten)]
    pub view: PluginView,
}

impl PluginActionOutput {
    pub fn new(view: PluginView) -> Self {
        Self { view }
    }
}

/// Shared view protocol returned by plugin actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum PluginView {
    Text(TextView),
    Markdown(MarkdownView),
    Html(HtmlView),
    Json(JsonView),
    Media(MediaView),
    BinaryUrl(BinaryUrlView),
    Table(TableView),
    Form(FormView),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextView {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownView {
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub html: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonView {
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaView {
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub encoding: PluginContentEncoding,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryUrlView {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableView {
    #[serde(default)]
    pub columns: Vec<TableColumn>,
    #[serde(default)]
    pub rows: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumn {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormView {
    pub schema: Value,
    #[serde(default)]
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit_action: Option<String>,
}
