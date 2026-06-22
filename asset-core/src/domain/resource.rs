use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ==================================================
// 核心聚合根
// ==================================================

crate::gen_id_uuid!(ResourceId);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: ResourceId,
    pub name: String,
    pub kind: ResourceKind,
    pub status: ResourceStatus,
    pub metadata: ResourceMetadata,
    pub content: Option<ResourceContent>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Resource {

    pub fn is_active(&self) -> bool {
        self.deleted_at.is_none() && matches!(self.status, ResourceStatus::Active)
    }

    pub fn is_archived(&self) -> bool {
        self.deleted_at.is_none() && matches!(self.status, ResourceStatus::Archived)
    }

    /// 是否已被软删除
    ///
    /// 当 `deleted_at` 字段不为空时，表示为已经软删除。
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

// ==================================================
// 资源类型
// ==================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceKind(String);

impl ResourceKind {
    /// 资源类型未知
    pub const UNKNOWN: &'static str = "core:unknown";

    /// 创建一个资源类型实例。支持传入 `String` 或 `&str`。
    ///
    /// 建议采用 `namespace:typename` 的命名规范防止冲突。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 获取内部原始字符串的只读借用（`&str`）。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Default for ResourceKind {
    /// 默认值： "UNKNOWN"。
    fn default() -> Self {
        Self(Self::UNKNOWN.to_string())
    }
}

/// 打印支持，便于在`format!("{kind}")`或`info!("{kind}")`中使用。
impl std::fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 将能转换成 `String` 的类型允许通过 `.into()` 转换为 `ResourceKind` 类型。
impl<T: Into<String>> From<T> for ResourceKind {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

/// 允许使用 `s.parse::<ResourceKind>()` 语法。
impl std::str::FromStr for ResourceKind {
    // 因为底层是 String，任何字符串解析都不会失败，故使用 Infallible
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

// ==================================================
// 资源状态
// ==================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceStatus {
    Active,
    Archived,
}

impl ResourceStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    pub fn is_archived(&self) -> bool {
        matches!(self, Self::Archived)
    }
}

impl Default for ResourceStatus {
    /// 默认值： "Active"。
    fn default() -> Self {
        Self::Active
    }
}

// ==================================================
// 资源metadata
// ==================================================

/// 用于承载资源的动态元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMetadata(pub Value);

impl ResourceMetadata {

    /// # 示例
    /// ```rust
    /// let val = serde_json::json!({ "tags": ["rust", "c"] });
    /// let metadata = ResourceMetadata::new(val);
    /// ```
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// 获取内部 `Value` 的只读引用。
    ///
    /// 适用于只需要读取元数据内容，而不转移所有权的场景。
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// 消费 `ResourceMetadata` 并返回内部的 `Value`。
    ///
    /// 该方法会转移所有权（Ownership）。
    pub fn into_value(self) -> Value {
        self.0
    }

    /// 检查元数据是否为空。
    pub fn is_empty(&self) -> bool {
        match &self.0 {
            Value::Object(map) => map.is_empty(),
            Value ::Null => true,
            _ => false,
        }
    }
}

impl Default for ResourceMetadata {
    /// 提供默认实现，初始为一个空的 JSON 对象（`{}`）。
    fn default() -> Self {
        Self(Value::Object(Default::default()))
    }
}

// ==================================================
// 资源内容
// ==================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    pub key: StorageKey,
    pub size: u64,
    pub mime_type: Option<String>,
    pub original_filename: Option<String>,
    pub checksum: Vec<Checksum>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey(String);

impl std::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checksum {
    pub kind: ChecksumKind,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumKind {
    Sha256,
}

