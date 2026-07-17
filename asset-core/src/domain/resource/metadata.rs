use super::{ResourceKind, normalize_required_text};
use crate::error::ResourceError;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// 元数据描述允许的最大字符数。
const MAX_METADATA_DESCRIPTION_LEN: usize = 1024;
/// 元数据标签允许的最大数量。
const MAX_METADATA_TAGS: usize = 64;
/// 单个元数据标签允许的最大字符数。
const MAX_METADATA_TAG_LEN: usize = 64;

/// 资源元数据。
///
/// `summary` 是核心稳定字段，关系化存储；`kind_metadata` 是某一资源类型拥有的扩展数据，
/// 由对应 kind 的 schema 解释。聚合根仍然只持有这一个整体值对象。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceMetadata {
    summary: ResourceSummaryMetadata,
    #[serde(deserialize_with = "deserialize_required_option")]
    kind_metadata: Option<ResourceKindMetadata>,
}

/// 资源核心摘要元数据。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ResourceSummaryMetadata {
    description: Option<String>,
    tags: Vec<ResourceTag>,
}

/// 已完成归一化和校验的资源标签。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ResourceTag(String);

/// 由具体资源类型解释的扩展元数据。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResourceKindMetadata {
    kind: ResourceKind,
    schema_version: u32,
    data: Map<String, Value>,
}

/// 对整体 metadata 的部分更新。
///
/// `summary = None` 表示不修改摘要；kind metadata 使用三态补丁，避免把“不修改”和
/// “明确清空”混为一谈。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResourceMetadataPatch {
    summary: Option<ResourceSummaryMetadata>,
    kind_metadata: ResourceKindMetadataPatch,
}

/// kind metadata 的三态更新指令。
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ResourceKindMetadataPatch {
    #[default]
    Unchanged,
    Replace(ResourceKindMetadata),
    Clear,
}

impl ResourceMetadata {
    /// 创建资源元数据构建器。
    pub fn builder() -> ResourceMetadataBuilder {
        ResourceMetadataBuilder::new()
    }

    /// 创建只含核心摘要的资源元数据。
    pub fn new(summary: ResourceSummaryMetadata) -> Self {
        Self {
            summary,
            kind_metadata: None,
        }
    }

    /// 从已经分别解码的持久化字段还原整体元数据。
    pub fn from_parts(
        summary: ResourceSummaryMetadata,
        kind_metadata: Option<ResourceKindMetadata>,
    ) -> Self {
        Self {
            summary,
            kind_metadata,
        }
    }

    /// 返回核心摘要元数据。
    pub fn summary(&self) -> &ResourceSummaryMetadata {
        &self.summary
    }

    /// 返回 kind 专属元数据。
    pub fn kind_metadata(&self) -> Option<&ResourceKindMetadata> {
        self.kind_metadata.as_ref()
    }

    /// 返回资源描述。
    pub fn description(&self) -> Option<&str> {
        self.summary.description()
    }

    /// 返回资源标签列表。
    pub fn tags(&self) -> &[ResourceTag] {
        self.summary.tags()
    }

    /// 检查元数据是否为空。
    pub fn is_empty(&self) -> bool {
        self.summary.is_empty() && self.kind_metadata.is_none()
    }

    /// 替换资源描述。
    pub fn set_description(&mut self, description: Option<String>) -> Result<(), ResourceError> {
        self.summary.set_description(description)
    }

    /// 追加标签。
    pub fn add_tag(&mut self, tag: impl Into<String>) -> Result<(), ResourceError> {
        self.summary.add_tag(tag)
    }

    /// 校验 kind 专属数据是否属于当前资源类型。
    pub fn validate_for_kind(&self, kind: &ResourceKind) -> Result<(), ResourceError> {
        if self
            .kind_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.kind() != kind)
        {
            return Err(ResourceError::InvalidFormat {
                field: "metadata.kind_metadata.kind",
                reason: "kind metadata must match the resource kind",
            });
        }

        Ok(())
    }

    /// 应用部分更新，并按资源类型验证更新结果。
    pub fn apply_patch(
        &mut self,
        patch: ResourceMetadataPatch,
        kind: &ResourceKind,
    ) -> Result<bool, ResourceError> {
        let mut updated = self.clone();

        if let Some(summary) = patch.summary {
            updated.summary = summary;
        }

        match patch.kind_metadata {
            ResourceKindMetadataPatch::Unchanged => {}
            ResourceKindMetadataPatch::Replace(metadata) => {
                updated.kind_metadata = Some(metadata);
            }
            ResourceKindMetadataPatch::Clear => updated.kind_metadata = None,
        }

        updated.validate_for_kind(kind)?;
        let changed = *self != updated;
        *self = updated;
        Ok(changed)
    }

    /// 清除仅对原资源类型有效的扩展数据。
    pub(crate) fn clear_kind_metadata(&mut self) -> bool {
        self.kind_metadata.take().is_some()
    }
}

impl Default for ResourceMetadata {
    fn default() -> Self {
        Self::new(ResourceSummaryMetadata::default())
    }
}

impl ResourceSummaryMetadata {
    /// 创建并校验核心摘要元数据。
    pub fn new(description: Option<String>, tags: Vec<String>) -> Result<Self, ResourceError> {
        let description = normalize_optional_metadata_text(
            "metadata.summary.description",
            description,
            MAX_METADATA_DESCRIPTION_LEN,
        )?;
        let tags = normalize_tags(tags)?;
        Ok(Self { description, tags })
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn tags(&self) -> &[ResourceTag] {
        &self.tags
    }

    pub fn is_empty(&self) -> bool {
        self.description.is_none() && self.tags.is_empty()
    }

    pub fn set_description(&mut self, description: Option<String>) -> Result<(), ResourceError> {
        self.description = normalize_optional_metadata_text(
            "metadata.summary.description",
            description,
            MAX_METADATA_DESCRIPTION_LEN,
        )?;
        Ok(())
    }

    pub fn add_tag(&mut self, tag: impl Into<String>) -> Result<(), ResourceError> {
        let tag = ResourceTag::try_new(tag)?;

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
}

impl<'de> Deserialize<'de> for ResourceSummaryMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSummary {
            #[serde(deserialize_with = "deserialize_required_option")]
            description: Option<String>,
            tags: Vec<String>,
        }

        let raw = RawSummary::deserialize(deserializer)?;
        Self::new(raw.description, raw.tags).map_err(serde::de::Error::custom)
    }
}

impl ResourceTag {
    pub fn try_new(value: impl Into<String>) -> Result<Self, ResourceError> {
        normalize_required_text("metadata.summary.tag", &value.into(), MAX_METADATA_TAG_LEN)
            .map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ResourceTag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl ResourceKindMetadata {
    /// 创建 kind 专属元数据。当前只校验公共信封，具体 data schema 留给 kind registry。
    pub fn new(
        kind: ResourceKind,
        schema_version: u32,
        data: Map<String, Value>,
    ) -> Result<Self, ResourceError> {
        kind.validate()?;
        if schema_version == 0 {
            return Err(ResourceError::InvalidFormat {
                field: "metadata.kind_metadata.schema_version",
                reason: "schema version must be greater than zero",
            });
        }
        Ok(Self {
            kind,
            schema_version,
            data,
        })
    }

    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn data(&self) -> &Map<String, Value> {
        &self.data
    }
}

impl<'de> Deserialize<'de> for ResourceKindMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawKindMetadata {
            kind: String,
            schema_version: u32,
            data: Map<String, Value>,
        }

        let raw = RawKindMetadata::deserialize(deserializer)?;
        let kind = ResourceKind::try_new(raw.kind).map_err(serde::de::Error::custom)?;
        Self::new(kind, raw.schema_version, raw.data).map_err(serde::de::Error::custom)
    }
}

impl ResourceMetadataPatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_summary(mut self, summary: ResourceSummaryMetadata) -> Self {
        self.summary = Some(summary);
        self
    }

    pub fn with_kind_metadata(mut self, metadata: ResourceKindMetadata) -> Self {
        self.kind_metadata = ResourceKindMetadataPatch::Replace(metadata);
        self
    }

    pub fn clear_kind_metadata(mut self) -> Self {
        self.kind_metadata = ResourceKindMetadataPatch::Clear;
        self
    }
}

/// 资源元数据构建器。
#[derive(Debug, Clone, Default)]
pub struct ResourceMetadataBuilder {
    description: Option<String>,
    tags: Vec<String>,
    kind_metadata: Option<ResourceKindMetadata>,
}

impl ResourceMetadataBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn with_tags<T, I>(mut self, tags: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = T>,
    {
        self.tags.extend(tags.into_iter().map(Into::into));
        self
    }

    pub fn with_kind_metadata(mut self, metadata: ResourceKindMetadata) -> Self {
        self.kind_metadata = Some(metadata);
        self
    }

    pub fn build(self) -> Result<ResourceMetadata, ResourceError> {
        let summary = ResourceSummaryMetadata::new(self.description, self.tags)?;
        Ok(ResourceMetadata::from_parts(summary, self.kind_metadata))
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

fn normalize_tags(tags: Vec<String>) -> Result<Vec<ResourceTag>, ResourceError> {
    let mut normalized = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = ResourceTag::try_new(tag)?;
        if !normalized.contains(&tag) {
            normalized.push(tag);
        }
    }

    if normalized.len() > MAX_METADATA_TAGS {
        return Err(ResourceError::TooLong {
            field: "metadata.summary.tags",
            max: MAX_METADATA_TAGS,
        });
    }
    Ok(normalized)
}
