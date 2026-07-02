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

/// How the host should deliver object content to an action handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResourceActionContentDelivery {
    #[default]
    Auto,
    Inline,
    Url,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    kinds: Vec<String>,
    #[serde(default)]
    content_delivery: ResourceActionContentDelivery,
    #[serde(default)]
    requires_content: bool,
}

/// Resource and content matching rules for an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceActionWhen {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    kinds: Vec<String>,
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
            kinds: Vec::new(),
            content_delivery: ResourceActionContentDelivery::Auto,
            requires_content: false,
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
        self.kinds = when.kinds.clone();
        self.when = when;
        self
    }

    pub fn with_kinds(mut self, kinds: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.kinds = normalize_kinds(kinds);
        self.when.kinds = self.kinds.clone();
        self
    }

    pub fn with_content_delivery(mut self, delivery: ResourceActionContentDelivery) -> Self {
        self.content_delivery = delivery;
        self
    }

    pub fn with_requires_content(mut self, requires_content: bool) -> Self {
        self.requires_content = requires_content;
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

    pub fn content_delivery(&self) -> ResourceActionContentDelivery {
        self.content_delivery
    }

    pub fn requires_content(&self) -> bool {
        self.requires_content
    }

    pub fn when(&self) -> &ResourceActionWhen {
        &self.when
    }

    pub fn kinds(&self) -> &[String] {
        &self.kinds
    }

    pub fn matches_content(&self, mime_type: Option<&str>, storage_key: Option<&str>) -> bool {
        self.when.matches(mime_type, storage_key)
    }

    pub fn matches_resource(
        &self,
        kind: &str,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> bool {
        self.when.matches_resource(kind, mime_type, storage_key)
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

    pub fn with_kinds(mut self, kinds: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.kinds = normalize_kinds(kinds);
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

    pub fn kinds(&self) -> &[String] {
        &self.kinds
    }

    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.mime_types.is_empty() && self.extensions.is_empty()
    }

    pub fn matches(&self, mime_type: Option<&str>, storage_key: Option<&str>) -> bool {
        self.matches_content(mime_type, storage_key)
    }

    pub fn matches_resource(
        &self,
        kind: &str,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> bool {
        if !self.kinds.is_empty()
            && !self
                .kinds
                .iter()
                .any(|expected| expected.eq_ignore_ascii_case(kind))
        {
            return false;
        }

        self.matches_content(mime_type, storage_key)
    }

    fn matches_content(&self, mime_type: Option<&str>, storage_key: Option<&str>) -> bool {
        if self.mime_types.is_empty() && self.extensions.is_empty() {
            return true;
        }

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

fn normalize_kinds(kinds: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    kinds
        .into_iter()
        .map(|value| value.into().trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_when_matches_kind_mime_and_extension() {
        let when = ResourceActionWhen::new()
            .with_kinds(["core:video"])
            .with_mime_types(["video/*"])
            .with_extensions(["mp4"]);

        assert!(when.matches_resource("core:video", Some("video/mp4"), Some("demo.bin")));
        assert!(when.matches_resource("CORE:VIDEO", None, Some("demo.mp4")));
        assert!(!when.matches_resource("core:image", Some("video/mp4"), Some("demo.mp4")));
        assert!(!when.matches_resource("core:video", Some("application/pdf"), Some("demo.pdf")));
    }
}
