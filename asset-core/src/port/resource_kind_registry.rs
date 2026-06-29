//! 资源类型注册表端口。
//!
//! 该端口描述核心/应用入口如何发现当前运行时支持的资源类型。默认实现可以是内置静态表，
//! 后续插件系统可以通过同一端口注册和暴露更多 kind。

use crate::domain::ResourceKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 资源动作标识。
///
/// 核心只保留少量内置动作的执行入口；插件可以声明自己的动作 ID，应用入口据此展示
/// 或交给插件运行时处理。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceAction(String);

impl ResourceAction {
    pub const DOWNLOAD_CONTENT: &'static str = "download_content";
    pub const READ: &'static str = "read";
    pub const VIEW_INLINE: &'static str = "view_inline";
    pub const PREVIEW: &'static str = "preview";
    pub const THUMBNAIL: &'static str = "thumbnail";

    /// 创建资源动作标识。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_string())
    }

    /// 返回动作稳定文本值。
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

/// 资源类型动作定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceActionDefinition {
    /// 动作 ID，例如 `read`、`preview`、`plugin:sync`。
    id: ResourceAction,
    /// 展示名称。
    label: String,
}

impl ResourceActionDefinition {
    /// 创建资源动作定义。
    pub fn new(id: impl Into<ResourceAction>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    /// 返回动作 ID。
    pub fn id(&self) -> &ResourceAction {
        &self.id
    }

    /// 返回展示名称。
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// 资源类型定义。
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceKindDefinition {
    /// 资源类型值，例如 `core:file`、`mindustry:mod`。
    kind: ResourceKind,
    /// 展示名称。
    label: String,
    /// 默认 kind metadata schema id。
    schema_id: Option<String>,
    /// kind metadata JSON schema。
    metadata_schema: Option<Value>,
    /// 是否支持对象内容。
    supports_content: bool,
    /// kind 支持的动作，例如 `read`、`thumbnail`、`plugin:sync`。
    actions: Vec<ResourceActionDefinition>,
    /// 定义来源，例如 `builtin`、`config` 或 `plugin:<id>`。
    source: String,
}

impl ResourceKindDefinition {
    /// 创建资源类型定义。
    pub fn new(
        kind: ResourceKind,
        label: impl Into<String>,
        schema_id: Option<String>,
        supports_content: bool,
    ) -> Self {
        Self::with_source(kind, label, schema_id, supports_content, "builtin")
    }

    /// 创建带来源的资源类型定义。
    pub fn with_source(
        kind: ResourceKind,
        label: impl Into<String>,
        schema_id: Option<String>,
        supports_content: bool,
        source: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            label: label.into(),
            schema_id,
            metadata_schema: None,
            supports_content,
            actions: Vec::new(),
            source: source.into(),
        }
    }

    /// 设置 kind metadata JSON schema。
    pub fn with_metadata_schema(mut self, metadata_schema: Option<Value>) -> Self {
        self.metadata_schema = metadata_schema;
        self
    }

    /// 设置 kind 支持的动作。
    pub fn with_actions(mut self, actions: Vec<ResourceActionDefinition>) -> Self {
        self.actions = actions;
        self
    }

    /// 返回资源类型。
    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    /// 返回展示名称。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 返回默认 schema id。
    pub fn schema_id(&self) -> Option<&str> {
        self.schema_id.as_deref()
    }

    /// 返回 kind metadata JSON schema。
    pub fn metadata_schema(&self) -> Option<&Value> {
        self.metadata_schema.as_ref()
    }

    /// 返回是否支持对象内容。
    pub fn supports_content(&self) -> bool {
        self.supports_content
    }

    /// 返回 kind 支持的动作。
    pub fn actions(&self) -> &[ResourceActionDefinition] {
        &self.actions
    }

    /// 判断 kind 是否支持指定动作。
    pub fn has_action(&self, action: impl AsRef<str>) -> bool {
        let action = action.as_ref();
        self.actions
            .iter()
            .any(|definition| definition.id().as_str() == action)
    }

    /// 返回定义来源。
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// 资源类型注册表。
pub trait ResourceKindRegistry: Send + Sync {
    /// 列出当前运行时支持的资源类型。
    fn list(&self) -> Vec<ResourceKindDefinition>;

    /// 按 kind 查找资源类型定义。
    fn get(&self, kind: &ResourceKind) -> Option<ResourceKindDefinition> {
        self.list()
            .into_iter()
            .find(|definition| definition.kind().as_str() == kind.as_str())
    }

    /// 判断资源类型是否受支持。
    fn supports(&self, kind: &ResourceKind) -> bool {
        self.get(kind).is_some()
    }
}
