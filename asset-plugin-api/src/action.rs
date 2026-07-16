use serde::{Deserialize, Deserializer, Serialize};

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
    Reference,
}

/// Resource action executor family after manifest capabilities are resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResourceActionExecutorKind {
    #[default]
    Builtin,
    Plugin,
}

/// Optional object content required in addition to the resource snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceActionOutputContract {
    pub view: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceActionUi {
    pub group: Option<String>,
    pub order: Option<i32>,
    pub locations: Vec<String>,
}

/// Content matching rules used by kind auto-detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ResourceContentMatcher {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mime_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extensions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceContentMatcherDocument {
    #[serde(default)]
    mime_types: Vec<String>,
    #[serde(default)]
    extensions: Vec<String>,
}

impl<'de> Deserialize<'de> for ResourceContentMatcher {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = ResourceContentMatcherDocument::deserialize(deserializer)?;
        if document
            .mime_types
            .iter()
            .chain(&document.extensions)
            .any(|value| value.trim().is_empty())
        {
            return Err(serde::de::Error::custom(
                "content matcher values must not be empty",
            ));
        }
        Ok(Self::new()
            .with_mime_types(document.mime_types)
            .with_extensions(document.extensions))
    }
}

/// Resource/action matching rules used by action availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceActionAppliesTo {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    kinds: Vec<String>,
    #[serde(default, flatten)]
    content: ResourceContentMatcher,
}

/// Resource kind action declaration after manifest capabilities are resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceActionDefinition {
    id: ResourceAction,
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    handler: Option<String>,
    #[serde(default)]
    access: ResourceActionAccess,
    #[serde(default, skip_serializing_if = "ResourceActionAppliesTo::is_empty")]
    applies_to: ResourceActionAppliesTo,
    #[serde(default)]
    executor: ResourceActionExecutorKind,
    #[serde(default)]
    requires: ResourceActionRequirements,
    #[serde(default)]
    output: ResourceActionOutputContract,
    #[serde(default)]
    ui: ResourceActionUi,
}

impl ResourceActionDefinition {
    pub fn new(id: impl Into<ResourceAction>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            handler: None,
            access: ResourceActionAccess::ReadOnly,
            applies_to: ResourceActionAppliesTo::default(),
            executor: ResourceActionExecutorKind::Builtin,
            requires: ResourceActionRequirements::default(),
            output: ResourceActionOutputContract::default(),
            ui: ResourceActionUi::default(),
        }
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_handler(mut self, handler: impl Into<String>) -> Self {
        self.handler = Some(handler.into());
        self
    }

    pub fn with_access(mut self, access: ResourceActionAccess) -> Self {
        self.access = access;
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

    pub fn with_executor(mut self, executor: ResourceActionExecutorKind) -> Self {
        self.executor = executor;
        self
    }

    pub fn with_requirements(mut self, requirements: ResourceActionRequirements) -> Self {
        self.requires = requirements;
        self
    }

    pub fn with_output(mut self, output: ResourceActionOutputContract) -> Self {
        self.output = output;
        self
    }

    pub fn with_ui(mut self, ui: ResourceActionUi) -> Self {
        self.ui = ui;
        self
    }

    pub fn id(&self) -> &ResourceAction {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn handler(&self) -> Option<&str> {
        self.handler.as_deref()
    }

    pub fn access(&self) -> ResourceActionAccess {
        self.access
    }

    pub fn executor(&self) -> ResourceActionExecutorKind {
        self.executor
    }

    pub fn requirements(&self) -> &ResourceActionRequirements {
        &self.requires
    }

    pub fn output(&self) -> &ResourceActionOutputContract {
        &self.output
    }

    pub fn ui(&self) -> &ResourceActionUi {
        &self.ui
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

impl ResourceContentMatcher {
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

    pub fn matches_content(&self, mime_type: Option<&str>, storage_key: Option<&str>) -> bool {
        if self.is_empty() {
            return true;
        }

        let mime_type = mime_type.map(|value| value.to_ascii_lowercase());
        if let Some(mime_type) = mime_type.as_deref()
            && self
                .mime_types
                .iter()
                .any(|expected| mime_matches(expected, mime_type))
        {
            return true;
        }

        let storage_key = storage_key.map(|value| value.to_ascii_lowercase());
        if let Some(storage_key) = storage_key.as_deref()
            && self
                .extensions
                .iter()
                .any(|extension| storage_key.ends_with(extension))
        {
            return true;
        }

        false
    }
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
        if !self.kinds.is_empty()
            && !self
                .kinds
                .iter()
                .any(|expected| expected.eq_ignore_ascii_case(kind))
        {
            return false;
        }

        self.content.matches_content(mime_type, storage_key)
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
    fn action_requirements_are_the_only_content_requirement_state() {
        let action = ResourceActionDefinition::new("preview", "Preview").with_requirements(
            ResourceActionRequirements {
                content: true,
                content_delivery: ResourceActionContentDelivery::Reference,
            },
        );

        let value = serde_json::to_value(action).unwrap();
        assert_eq!(value["requires"]["content"], true);
        assert_eq!(value["requires"]["content_delivery"], "reference");
        assert!(value["requires"].get("resource").is_none());
        assert!(value["requires"].get("metadata").is_none());
        assert!(value.get("requires_content").is_none());
        assert!(value.get("content_delivery").is_none());
    }

    #[test]
    fn action_applies_to_matches_kind_mime_and_extension() {
        let applies_to = ResourceActionAppliesTo::new()
            .with_kinds(["core:video"])
            .with_mime_types(["video/*"])
            .with_extensions(["mp4"]);

        assert!(applies_to.matches_resource("core:video", Some("video/mp4"), Some("demo.bin")));
        assert!(applies_to.matches_resource("CORE:VIDEO", None, Some("demo.mp4")));
        assert!(!applies_to.matches_resource("core:image", Some("video/mp4"), Some("demo.mp4")));
        assert!(!applies_to.matches_resource(
            "core:video",
            Some("application/pdf"),
            Some("demo.pdf")
        ));
    }

    #[test]
    fn matcher_deserialization_preserves_normalized_invariants() {
        let matcher: ResourceContentMatcher = serde_json::from_value(serde_json::json!({
            "mime_types": [" Text/Markdown "],
            "extensions": ["MD"]
        }))
        .unwrap();

        assert_eq!(matcher.mime_types(), ["text/markdown"]);
        assert_eq!(matcher.extensions(), [".md"]);
        assert!(matcher.matches_content(Some("TEXT/MARKDOWN"), None));
        assert!(matcher.matches_content(None, Some("README.MD")));
        assert!(
            serde_json::from_value::<ResourceContentMatcher>(serde_json::json!({
                "mime_types": ["  "]
            }))
            .is_err()
        );
    }
}
