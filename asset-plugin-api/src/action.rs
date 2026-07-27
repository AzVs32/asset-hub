use serde::{Deserialize, Deserializer, Serialize};

/// Action identifier shared by resource and directory targets.
///
/// Action IDs are extensible. Authors are encouraged to use
/// `<plugin-id>.<verb>` (for example `core.resource.download` or
/// `azvs.markdown.render`) so independently contributed actions remain globally distinct.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionId(String);

impl ActionId {
    /// Built-in download capability contributed by the `core.resource` plugin.
    pub const CORE_RESOURCE_DOWNLOAD: &'static str = "core.resource.download";
    /// Built-in ZIP download capability contributed by the `core.directory` plugin.
    pub const CORE_DIRECTORY_DOWNLOAD: &'static str = "core.directory.download";

    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_string())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for ActionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ActionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ActionId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Resource action access boundary used by host execution requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActionAccess {
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
pub enum ActionExecutorKind {
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
pub struct ActionOutputContract {
    pub view: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionUi {
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

/// Target-independent action declaration fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionDefinition {
    id: ActionId,
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    handler: Option<String>,
    #[serde(default)]
    access: ActionAccess,
    #[serde(default)]
    executor: ActionExecutorKind,
    #[serde(default)]
    output: ActionOutputContract,
    #[serde(default)]
    ui: ActionUi,
}

impl ActionDefinition {
    pub fn new(id: impl Into<ActionId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            handler: None,
            access: ActionAccess::ReadOnly,
            executor: ActionExecutorKind::Builtin,
            output: ActionOutputContract::default(),
            ui: ActionUi::default(),
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

    pub fn with_access(mut self, access: ActionAccess) -> Self {
        self.access = access;
        self
    }

    pub fn with_executor(mut self, executor: ActionExecutorKind) -> Self {
        self.executor = executor;
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

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn handler(&self) -> Option<&str> {
        self.handler.as_deref()
    }

    pub fn access(&self) -> ActionAccess {
        self.access
    }

    pub fn executor(&self) -> ActionExecutorKind {
        self.executor
    }

    pub fn output(&self) -> &ActionOutputContract {
        &self.output
    }

    pub fn ui(&self) -> &ActionUi {
        &self.ui
    }
}

/// Resource action declaration after manifest capabilities are resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceActionDefinition {
    #[serde(flatten)]
    action: ActionDefinition,
    #[serde(default, skip_serializing_if = "ResourceActionAppliesTo::is_empty")]
    applies_to: ResourceActionAppliesTo,
    #[serde(default)]
    requires: ResourceActionRequirements,
}

impl ResourceActionDefinition {
    pub fn new(id: impl Into<ActionId>, label: impl Into<String>) -> Self {
        Self {
            action: ActionDefinition::new(id, label),
            applies_to: ResourceActionAppliesTo::default(),
            requires: ResourceActionRequirements::default(),
        }
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.action = self.action.with_description(description);
        self
    }
    pub fn with_handler(mut self, handler: impl Into<String>) -> Self {
        self.action = self.action.with_handler(handler);
        self
    }
    pub fn with_access(mut self, access: ActionAccess) -> Self {
        self.action = self.action.with_access(access);
        self
    }
    pub fn with_executor(mut self, executor: ActionExecutorKind) -> Self {
        self.action = self.action.with_executor(executor);
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
    pub fn label(&self) -> &str {
        self.action.label()
    }
    pub fn description(&self) -> Option<&str> {
        self.action.description()
    }
    pub fn handler(&self) -> Option<&str> {
        self.action.handler()
    }
    pub fn access(&self) -> ActionAccess {
        self.action.access()
    }
    pub fn executor(&self) -> ActionExecutorKind {
        self.action.executor()
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

/// Directory/action matching rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DirectoryActionAppliesTo {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DirectoryActionRequirements {
    #[serde(default)]
    pub children: bool,
    #[serde(default)]
    pub resources: bool,
}

/// Directory action declaration after manifest capabilities are resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryActionDefinition {
    #[serde(flatten)]
    action: ActionDefinition,
    #[serde(default, skip_serializing_if = "DirectoryActionAppliesTo::is_empty")]
    applies_to: DirectoryActionAppliesTo,
    #[serde(default)]
    requires: DirectoryActionRequirements,
}

impl DirectoryActionDefinition {
    pub fn new(id: impl Into<ActionId>, label: impl Into<String>) -> Self {
        Self {
            action: ActionDefinition::new(id, label),
            applies_to: DirectoryActionAppliesTo::default(),
            requires: DirectoryActionRequirements::default(),
        }
    }
    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.action = self.action.with_description(description);
        self
    }
    pub fn with_handler(mut self, handler: impl Into<String>) -> Self {
        self.action = self.action.with_handler(handler);
        self
    }
    pub fn with_access(mut self, access: ActionAccess) -> Self {
        self.action = self.action.with_access(access);
        self
    }
    pub fn with_executor(mut self, executor: ActionExecutorKind) -> Self {
        self.action = self.action.with_executor(executor);
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
    pub fn label(&self) -> &str {
        self.action.label()
    }
    pub fn description(&self) -> Option<&str> {
        self.action.description()
    }
    pub fn handler(&self) -> Option<&str> {
        self.action.handler()
    }
    pub fn access(&self) -> ActionAccess {
        self.action.access()
    }
    pub fn executor(&self) -> ActionExecutorKind {
        self.action.executor()
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

pub type ResourceAction = ActionId;
pub type DirectoryAction = ActionId;
pub type ResourceActionAccess = ActionAccess;
pub type DirectoryActionAccess = ActionAccess;
pub type ResourceActionExecutorKind = ActionExecutorKind;
pub type DirectoryActionExecutorKind = ActionExecutorKind;
pub type ResourceActionOutputContract = ActionOutputContract;
pub type DirectoryActionOutputContract = ActionOutputContract;
pub type ResourceActionUi = ActionUi;
pub type DirectoryActionUi = ActionUi;

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
mod tests;
