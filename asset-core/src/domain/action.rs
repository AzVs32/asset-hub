//! Host 内部的 Action 注册、匹配、授权与执行模型。
//!
//! 外部 Manifest 由基础设施适配器显式转换为这些定义；本模块不属于插件 SDK，
//! 也不描述 Host 与插件之间的 JSON 线协议。

/// Action identifier shared by resource and directory targets.
///
/// Action IDs are extensible and globally unique. External Manifest adapters preserve validated
/// `<plugin-id>.<verb>` identifiers when constructing this value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionId(String);

impl ActionId {
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

/// Semantic singleton capability implemented by one of several action providers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionCapabilityId(String);

impl ActionCapabilityId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_string())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for ActionCapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for ActionCapabilityId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ActionCapabilityId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ActionCapabilityId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Resource action access boundary used by host execution requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionAccess {
    #[default]
    ReadOnly,
    ReadWrite,
}

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionOutputContract {
    pub view: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionUi {
    pub group: Option<String>,
    pub order: Option<i32>,
    pub locations: Vec<String>,
}

/// Content matching rules used by kind auto-detection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceContentMatcher {
    mime_types: Vec<String>,
    extensions: Vec<String>,
}

/// Resource/action matching rules used by action availability.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceActionAppliesTo {
    kinds: Vec<String>,
    content: ResourceContentMatcher,
}

/// Target-independent action declaration fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDefinition {
    id: ActionId,
    provides: Option<ActionCapabilityId>,
    label: String,
    description: Option<String>,
    access: ActionAccess,
    output: ActionOutputContract,
    ui: ActionUi,
}

impl ActionDefinition {
    pub fn new(id: impl Into<ActionId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provides: None,
            label: label.into(),
            description: None,
            access: ActionAccess::ReadOnly,
            output: ActionOutputContract::default(),
            ui: ActionUi::default(),
        }
    }

    pub fn with_provides(mut self, provides: Option<impl Into<ActionCapabilityId>>) -> Self {
        self.provides = provides.map(Into::into);
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

/// Resource action declaration after manifest capabilities are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceActionDefinition {
    action: ActionDefinition,
    applies_to: ResourceActionAppliesTo,
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
    pub fn with_provides(mut self, provides: Option<impl Into<ActionCapabilityId>>) -> Self {
        self.action = self.action.with_provides(provides);
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
    pub fn with_provides(mut self, provides: Option<impl Into<ActionCapabilityId>>) -> Self {
        self.action = self.action.with_provides(provides);
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

pub type ResourceAction = ActionId;
pub type DirectoryAction = ActionId;
pub type ResourceActionAccess = ActionAccess;
pub type DirectoryActionAccess = ActionAccess;
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
