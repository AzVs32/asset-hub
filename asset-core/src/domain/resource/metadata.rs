use super::{ResourceKind, normalize_required_text};
use crate::error::ResourceError;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// 元数据描述允许的最大字符数。
const MAX_METADATA_DESCRIPTION_LEN: usize = 1024;
/// 元数据标签允许的最大数量。
const MAX_METADATA_TAGS: usize = 64;
/// 单个元数据标签允许的最大字符数。
const MAX_METADATA_TAG_LEN: usize = 64;
/// 单个资源允许保存的 kind metadata 层数上限，防止恶意构造超深谱系。
const MAX_KIND_METADATA_LAYERS: usize = 32;

/// 资源元数据。
///
/// `summary` 是核心稳定字段，关系化存储；`kind_metadata` 是当前资源 kind 谱系上各个
/// kind 独立拥有的扩展数据集合。每层数据都由其 owner kind 的 schema 单独解释。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceMetadata {
    summary: ResourceSummaryMetadata,
    kind_metadata: ResourceKindMetadataSet,
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

/// 一个资源当前生效的全部 kind metadata 层。
///
/// 集合按 owner kind 的规范字符串排序，因此相等性、序列化和仓储 round-trip 都不依赖
/// 插入顺序。HTTP 展示层可再按照当前 kind lineage 重排为 root -> leaf。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ResourceKindMetadataSet {
    layers: Vec<ResourceKindMetadata>,
}

/// 对整体 metadata 的部分更新。
///
/// `summary = None` 表示不修改摘要；kind metadata 使用逐层 upsert/clear，避免更新一层
/// 时覆盖谱系上的其他层。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResourceMetadataPatch {
    summary: Option<ResourceSummaryMetadata>,
    kind_metadata: ResourceKindMetadataPatch,
}

/// kind metadata 的逐层更新指令。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResourceKindMetadataPatch {
    upsert: Vec<ResourceKindMetadata>,
    clear: Vec<ResourceKind>,
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
            kind_metadata: ResourceKindMetadataSet::default(),
        }
    }

    /// 从已经分别解码的持久化字段还原整体元数据。
    pub fn from_parts(
        summary: ResourceSummaryMetadata,
        kind_metadata: ResourceKindMetadataSet,
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
    pub fn kind_metadata(&self) -> &ResourceKindMetadataSet {
        &self.kind_metadata
    }

    /// 返回指定 owner kind 的 metadata 层。
    pub fn kind_metadata_for(&self, kind: &ResourceKind) -> Option<&ResourceKindMetadata> {
        self.kind_metadata.get(kind)
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
        self.summary.is_empty() && self.kind_metadata.is_empty()
    }

    /// 替换资源描述。
    pub fn set_description(&mut self, description: Option<String>) -> Result<(), ResourceError> {
        self.summary.set_description(description)
    }

    /// 追加标签。
    pub fn add_tag(&mut self, tag: impl Into<String>) -> Result<(), ResourceError> {
        self.summary.add_tag(tag)
    }

    /// 校验所有 layer owner 都属于当前资源 kind 的谱系。
    ///
    /// lineage 的顺序不参与语义；registry 当前提供 leaf -> root，HTTP 可按需反转。
    pub fn validate_for_lineage(&self, lineage: &[ResourceKind]) -> Result<(), ResourceError> {
        self.kind_metadata.validate_for_lineage(lineage)
    }

    /// 应用部分更新，并按资源类型验证更新结果。
    pub fn apply_patch(
        &mut self,
        patch: ResourceMetadataPatch,
        lineage: &[ResourceKind],
    ) -> Result<bool, ResourceError> {
        let mut updated = self.clone();

        if let Some(summary) = patch.summary {
            updated.summary = summary;
        }

        patch.kind_metadata.apply_to(&mut updated.kind_metadata)?;
        updated.validate_for_lineage(lineage)?;
        let changed = *self != updated;
        *self = updated;
        Ok(changed)
    }

    /// 只保留新 lineage 中仍然有效的 layer，用于 kind 切换时保留共同祖先数据。
    pub(crate) fn retain_kind_metadata_for_lineage(&mut self, lineage: &[ResourceKind]) -> bool {
        self.kind_metadata.retain_for_lineage(lineage)
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

impl ResourceKindMetadataSet {
    /// 创建一个按 owner kind 规范化的 metadata 集合。
    pub fn new(layers: Vec<ResourceKindMetadata>) -> Result<Self, ResourceError> {
        if layers.len() > MAX_KIND_METADATA_LAYERS {
            return Err(ResourceError::InvalidKindMetadata {
                kind: "*".to_owned(),
                reason: format!("layer count must not exceed {MAX_KIND_METADATA_LAYERS}"),
            });
        }

        let mut layers = layers;
        layers.sort_by(|left, right| left.kind().as_str().cmp(right.kind().as_str()));
        if let Some(duplicate) = layers
            .windows(2)
            .find(|pair| pair[0].kind() == pair[1].kind())
            .map(|pair| pair[0].kind().as_str().to_owned())
        {
            return Err(ResourceError::InvalidKindMetadata {
                kind: duplicate,
                reason: "duplicate owner kind".to_owned(),
            });
        }

        Ok(Self { layers })
    }

    pub fn layers(&self) -> &[ResourceKindMetadata] {
        &self.layers
    }

    pub fn get(&self, kind: &ResourceKind) -> Option<&ResourceKindMetadata> {
        self.layers
            .binary_search_by(|layer| layer.kind().as_str().cmp(kind.as_str()))
            .ok()
            .map(|index| &self.layers[index])
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    fn upsert(&mut self, metadata: ResourceKindMetadata) {
        match self
            .layers
            .binary_search_by(|layer| layer.kind().as_str().cmp(metadata.kind().as_str()))
        {
            Ok(index) => self.layers[index] = metadata,
            Err(index) => self.layers.insert(index, metadata),
        }
    }

    fn remove(&mut self, kind: &ResourceKind) -> bool {
        let Ok(index) = self
            .layers
            .binary_search_by(|layer| layer.kind().as_str().cmp(kind.as_str()))
        else {
            return false;
        };
        self.layers.remove(index);
        true
    }

    fn validate_for_lineage(&self, lineage: &[ResourceKind]) -> Result<(), ResourceError> {
        for layer in &self.layers {
            if !lineage.iter().any(|kind| kind == layer.kind()) {
                return Err(ResourceError::InvalidKindMetadata {
                    kind: layer.kind().as_str().to_owned(),
                    reason: "owner kind is not in the resource kind lineage".to_owned(),
                });
            }
        }
        Ok(())
    }

    fn retain_for_lineage(&mut self, lineage: &[ResourceKind]) -> bool {
        let previous_len = self.layers.len();
        self.layers
            .retain(|layer| lineage.iter().any(|kind| kind == layer.kind()));
        self.layers.len() != previous_len
    }
}

impl<'de> Deserialize<'de> for ResourceKindMetadataSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawKindMetadataSet {
            layers: Vec<ResourceKindMetadata>,
        }

        let raw = RawKindMetadataSet::deserialize(deserializer)?;
        Self::new(raw.layers).map_err(serde::de::Error::custom)
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

impl ResourceKindMetadataPatch {
    pub fn upserts(&self) -> &[ResourceKindMetadata] {
        &self.upsert
    }

    pub fn cleared_kinds(&self) -> &[ResourceKind] {
        &self.clear
    }

    fn validate(&self) -> Result<(), ResourceError> {
        let mut upsert = HashSet::new();
        for layer in &self.upsert {
            if !upsert.insert(layer.kind().as_str()) {
                return Err(ResourceError::InvalidKindMetadata {
                    kind: layer.kind().as_str().to_owned(),
                    reason: "duplicate upsert operation".to_owned(),
                });
            }
        }

        let mut clear = HashSet::new();
        for kind in &self.clear {
            if !clear.insert(kind.as_str()) {
                return Err(ResourceError::InvalidKindMetadata {
                    kind: kind.as_str().to_owned(),
                    reason: "duplicate clear operation".to_owned(),
                });
            }
            if upsert.contains(kind.as_str()) {
                return Err(ResourceError::InvalidKindMetadata {
                    kind: kind.as_str().to_owned(),
                    reason: "the same layer cannot be upserted and cleared".to_owned(),
                });
            }
        }
        Ok(())
    }

    fn apply_to(self, metadata: &mut ResourceKindMetadataSet) -> Result<(), ResourceError> {
        self.validate()?;
        for kind in self.clear {
            metadata.remove(&kind);
        }
        for layer in self.upsert {
            metadata.upsert(layer);
        }
        if metadata.layers.len() > MAX_KIND_METADATA_LAYERS {
            return Err(ResourceError::InvalidKindMetadata {
                kind: "*".to_owned(),
                reason: format!("layer count must not exceed {MAX_KIND_METADATA_LAYERS}"),
            });
        }
        Ok(())
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

    pub fn kind_metadata_patch(&self) -> &ResourceKindMetadataPatch {
        &self.kind_metadata
    }

    pub fn with_kind_metadata(mut self, metadata: ResourceKindMetadata) -> Self {
        self.kind_metadata.upsert.push(metadata);
        self
    }

    /// 清除单个 owner kind 的 metadata 层。
    pub fn clear_kind_metadata_for(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind_metadata.clear.push(kind.into());
        self
    }
}

/// 资源元数据构建器。
#[derive(Debug, Clone, Default)]
pub struct ResourceMetadataBuilder {
    description: Option<String>,
    tags: Vec<String>,
    kind_metadata: Vec<ResourceKindMetadata>,
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
        self.kind_metadata.push(metadata);
        self
    }

    pub fn build(self) -> Result<ResourceMetadata, ResourceError> {
        let summary = ResourceSummaryMetadata::new(self.description, self.tags)?;
        let kind_metadata = ResourceKindMetadataSet::new(self.kind_metadata)?;
        Ok(ResourceMetadata::from_parts(summary, kind_metadata))
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
