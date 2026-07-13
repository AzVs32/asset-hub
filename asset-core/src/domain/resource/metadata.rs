use super::normalize_required_text;
use crate::error::ResourceError;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// 当前资源元数据结构版本。
const RESOURCE_METADATA_SCHEMA_VERSION: u32 = 1;
/// 元数据描述允许的最大字符数。
const MAX_METADATA_DESCRIPTION_LEN: usize = 1024;
/// 元数据标签允许的最大数量。
const MAX_METADATA_TAGS: usize = 64;
/// 单个元数据标签允许的最大字符数。
const MAX_METADATA_TAG_LEN: usize = 64;
/// kind metadata schema id 允许的最大字符数。
const MAX_KIND_SCHEMA_ID_LEN: usize = 128;

/// 资源元数据。
///
/// 元数据分为核心摘要和 kind 专属扩展两层：
/// - `summary` 由 Asset Hub 核心统一理解，用于描述、标签、查询和基础展示。
/// - `kind` 预留给插件定义 schema 与数据，核心层只要求其是带 schema id 的 JSON object。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceMetadata {
    /// 元数据结构版本，由服务端维护。
    schema_version: u32,
    /// 核心摘要元数据。
    summary: ResourceSummaryMetadata,
    /// kind/plugin 专属元数据。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<KindMetadata>,
}

/// 资源核心摘要元数据。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSummaryMetadata {
    /// 资源描述。
    #[serde(deserialize_with = "deserialize_required_option")]
    description: Option<String>,
    /// 资源标签。
    tags: Vec<String>,
}

/// kind/plugin 专属元数据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindMetadata {
    /// 插件 schema 标识，例如 `mindustry:mod@1`。
    schema_id: String,
    /// 经过该 schema 解释的 JSON object。
    data: Value,
}

impl ResourceMetadata {
    /// 返回当前元数据结构版本。
    pub const fn current_schema_version() -> u32 {
        RESOURCE_METADATA_SCHEMA_VERSION
    }

    /// 创建资源元数据构建器。
    pub fn builder() -> ResourceMetadataBuilder {
        ResourceMetadataBuilder::new()
    }

    /// 创建并校验资源元数据。
    pub fn new(
        summary: ResourceSummaryMetadata,
        kind: Option<KindMetadata>,
    ) -> Result<Self, ResourceError> {
        let summary = summary.validate()?;
        let kind = kind.map(KindMetadata::validate).transpose()?;

        Ok(Self {
            schema_version: RESOURCE_METADATA_SCHEMA_VERSION,
            summary,
            kind,
        })
    }

    /// 从持久化 JSON 值还原资源元数据。
    ///
    /// 只接受当前结构，不再兼容历史自由 JSON 格式。
    pub fn from_persisted_value(value: Value) -> Result<Self, ResourceError> {
        serde_json::from_value::<Self>(value)
            .map_err(|_| ResourceError::InvalidFormat {
                field: "metadata",
                reason: "metadata does not match the current schema",
            })
            .and_then(Self::validate)
    }

    /// 返回元数据结构版本。
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// 返回核心摘要元数据。
    pub fn summary(&self) -> &ResourceSummaryMetadata {
        &self.summary
    }

    /// 返回 kind/plugin 专属元数据。
    pub fn kind_metadata(&self) -> Option<&KindMetadata> {
        self.kind.as_ref()
    }

    /// 返回资源描述。
    pub fn description(&self) -> Option<&str> {
        self.summary.description()
    }

    /// 返回资源标签列表。
    pub fn tags(&self) -> &[String] {
        self.summary.tags()
    }

    /// 转换为 JSON 值。
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("resource metadata serialization should not fail")
    }

    /// 检查元数据是否为空。
    pub fn is_empty(&self) -> bool {
        self.summary.is_empty() && self.kind.is_none()
    }

    /// 替换资源描述。
    pub fn set_description(&mut self, description: Option<String>) -> Result<(), ResourceError> {
        self.summary.set_description(description)
    }

    /// 追加标签。
    pub fn add_tag(&mut self, tag: impl Into<String>) -> Result<(), ResourceError> {
        self.summary.add_tag(tag)
    }

    fn validate(self) -> Result<Self, ResourceError> {
        if self.schema_version != RESOURCE_METADATA_SCHEMA_VERSION {
            return Err(ResourceError::InvalidFormat {
                field: "metadata.schema_version",
                reason: "unsupported resource metadata schema version",
            });
        }

        Self::new(self.summary, self.kind)
    }
}

impl Default for ResourceMetadata {
    fn default() -> Self {
        Self {
            schema_version: RESOURCE_METADATA_SCHEMA_VERSION,
            summary: ResourceSummaryMetadata::default(),
            kind: None,
        }
    }
}

impl ResourceSummaryMetadata {
    /// 创建核心摘要元数据。
    pub fn new(description: Option<String>, tags: Vec<String>) -> Result<Self, ResourceError> {
        Self { description, tags }.validate()
    }

    /// 返回资源描述。
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// 返回资源标签列表。
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// 检查摘要是否为空。
    pub fn is_empty(&self) -> bool {
        self.description.is_none() && self.tags.is_empty()
    }

    /// 替换资源描述。
    pub fn set_description(&mut self, description: Option<String>) -> Result<(), ResourceError> {
        self.description = normalize_optional_metadata_text(
            "metadata.summary.description",
            description,
            MAX_METADATA_DESCRIPTION_LEN,
        )?;
        Ok(())
    }

    /// 追加标签。
    pub fn add_tag(&mut self, tag: impl Into<String>) -> Result<(), ResourceError> {
        let tag =
            normalize_required_text("metadata.summary.tag", &tag.into(), MAX_METADATA_TAG_LEN)?;

        if self.tags.len() >= MAX_METADATA_TAGS && !self.tags.contains(&tag) {
            return Err(ResourceError::TooLong {
                field: "metadata.summary.tags",
                max: MAX_METADATA_TAGS,
            });
        }

        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }

        Ok(())
    }

    fn validate(self) -> Result<Self, ResourceError> {
        let description = normalize_optional_metadata_text(
            "metadata.summary.description",
            self.description,
            MAX_METADATA_DESCRIPTION_LEN,
        )?;
        let tags = normalize_tags(self.tags)?;

        Ok(Self { description, tags })
    }
}

impl KindMetadata {
    /// 创建 kind/plugin 专属元数据。
    pub fn new(schema_id: impl Into<String>, data: Value) -> Result<Self, ResourceError> {
        Self {
            schema_id: schema_id.into(),
            data,
        }
        .validate()
    }

    /// 返回 schema id。
    pub fn schema_id(&self) -> &str {
        &self.schema_id
    }

    /// 返回 kind/plugin 专属数据。
    pub fn data(&self) -> &Value {
        &self.data
    }

    fn validate(self) -> Result<Self, ResourceError> {
        let schema_id = normalize_required_text(
            "metadata.kind.schema_id",
            &self.schema_id,
            MAX_KIND_SCHEMA_ID_LEN,
        )?;

        if schema_id.chars().any(char::is_whitespace) {
            return Err(ResourceError::InvalidFormat {
                field: "metadata.kind.schema_id",
                reason: "whitespace is not allowed",
            });
        }

        if !self.data.is_object() {
            return Err(ResourceError::InvalidFormat {
                field: "metadata.kind.data",
                reason: "kind metadata data must be a JSON object",
            });
        }

        Ok(Self {
            schema_id,
            data: self.data,
        })
    }
}

/// 资源元数据构建器。
#[derive(Debug, Clone, Default)]
pub struct ResourceMetadataBuilder {
    /// 核心摘要描述。
    description: Option<String>,
    /// 核心摘要标签。
    tags: Vec<String>,
    /// kind/plugin 专属元数据。
    kind: Option<KindMetadata>,
}

impl ResourceMetadataBuilder {
    /// 创建资源元数据构建器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置资源描述。
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 追加一个资源标签。
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// 批量追加资源标签。
    pub fn with_tags<T, I>(mut self, tags: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = T>,
    {
        self.tags.extend(tags.into_iter().map(Into::into));
        self
    }

    /// 设置 kind/plugin 专属元数据。
    pub fn with_kind_metadata(mut self, kind: KindMetadata) -> Self {
        self.kind = Some(kind);
        self
    }

    /// 完成构建并执行元数据校验。
    pub fn build(self) -> Result<ResourceMetadata, ResourceError> {
        let summary = ResourceSummaryMetadata {
            description: self.description,
            tags: self.tags,
        };

        ResourceMetadata::new(summary, self.kind)
    }
}

fn normalize_optional_metadata_text(
    field: &'static str,
    value: Option<String>,
    max: usize,
) -> Result<Option<String>, ResourceError> {
    value
        .map(|value| normalize_required_text(field, &value, max))
        .transpose()
}

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn normalize_tags(tags: Vec<String>) -> Result<Vec<String>, ResourceError> {
    if tags.len() > MAX_METADATA_TAGS {
        return Err(ResourceError::TooLong {
            field: "metadata.summary.tags",
            max: MAX_METADATA_TAGS,
        });
    }

    let mut normalized = Vec::with_capacity(tags.len());

    for tag in tags {
        let tag = normalize_required_text("metadata.summary.tag", &tag, MAX_METADATA_TAG_LEN)?;

        if !normalized.contains(&tag) {
            normalized.push(tag);
        }
    }

    Ok(normalized)
}
