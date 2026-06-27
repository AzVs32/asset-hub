use super::normalize_required_text;
use crate::error::ResourceError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 当前资源元数据结构版本。
const RESOURCE_METADATA_SCHEMA_VERSION: u32 = 1;
/// 元数据描述允许的最大字符数。
const MAX_METADATA_DESCRIPTION_LEN: usize = 1024;
/// 元数据标签允许的最大数量。
const MAX_METADATA_TAGS: usize = 64;
/// 单个元数据标签允许的最大字符数。
const MAX_METADATA_TAG_LEN: usize = 64;
/// 扩展属性键允许的最大字符数。
const MAX_METADATA_ATTRIBUTE_KEY_LEN: usize = 255;

// ==================================================
// 资源metadata
// ==================================================

/// 资源元数据。
///
/// 元数据由服务端定义稳定结构，调用方不能把任意 JSON 直接作为整段元数据写入。
/// `attributes` 是预留的扩展字段，用于承载暂时没有被提升为一等字段的业务属性。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceMetadata {
    /// 元数据结构版本，由服务端维护。
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    /// 资源描述。
    #[serde(default)]
    description: Option<String>,
    /// 资源标签。
    #[serde(default)]
    tags: Vec<String>,
    /// 资源扩展属性。
    #[serde(default)]
    attributes: BTreeMap<String, Value>,
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
        description: Option<String>,
        tags: Vec<String>,
        attributes: BTreeMap<String, Value>,
    ) -> Result<Self, ResourceError> {
        let description = normalize_optional_metadata_text(
            "metadata.description",
            description,
            MAX_METADATA_DESCRIPTION_LEN,
        )?;
        let tags = normalize_tags(tags)?;
        let attributes = normalize_attributes(attributes)?;

        Ok(Self {
            schema_version: RESOURCE_METADATA_SCHEMA_VERSION,
            description,
            tags,
            attributes,
        })
    }

    /// 从持久化 JSON 值还原资源元数据。
    ///
    /// 当前版本的持久化格式是包含 `schema_version`、`description`、`tags` 和
    /// `attributes` 的对象。为了兼容早期保存的任意 JSON object，如果对象没有任何
    /// 当前结构字段，会把它整体迁移到 `attributes` 中。
    pub fn from_persisted_value(value: Value) -> Result<Self, ResourceError> {
        match value {
            Value::Null => Ok(Self::default()),
            Value::Object(mut object) => {
                if is_structured_metadata_object(&object) {
                    let schema_version = object
                        .remove("schema_version")
                        .map(parse_schema_version)
                        .transpose()?
                        .unwrap_or(RESOURCE_METADATA_SCHEMA_VERSION);
                    let description = object
                        .remove("description")
                        .map(parse_description)
                        .transpose()?
                        .flatten();
                    let tags = object
                        .remove("tags")
                        .map(parse_tags)
                        .transpose()?
                        .unwrap_or_default();
                    let mut attributes = object
                        .remove("attributes")
                        .map(parse_attributes)
                        .transpose()?
                        .unwrap_or_default();

                    attributes.extend(object);

                    Self {
                        schema_version,
                        description,
                        tags,
                        attributes,
                    }
                    .validate()
                } else {
                    let attributes: BTreeMap<String, Value> = object.into_iter().collect();
                    Self::builder().with_attributes(attributes).build()
                }
            }
            _ => Err(ResourceError::InvalidFormat {
                field: "metadata",
                reason: "metadata must be a JSON object",
            }),
        }
    }

    /// 返回元数据结构版本。
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// 返回资源描述。
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// 返回资源标签列表。
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// 返回扩展属性集合。
    pub fn attributes(&self) -> &BTreeMap<String, Value> {
        &self.attributes
    }

    /// 按 key 读取扩展属性。
    pub fn attribute(&self, key: &str) -> Option<&Value> {
        self.attributes.get(key)
    }

    /// 消费 `ResourceMetadata` 并返回 JSON 值。
    pub fn into_value(self) -> Value {
        serde_json::to_value(self).expect("resource metadata serialization should not fail")
    }

    /// 转换为 JSON 值。
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("resource metadata serialization should not fail")
    }

    /// 检查元数据是否为空。
    ///
    /// 结构版本不参与空判断；只要描述、标签和扩展属性都为空，就视为空元数据。
    pub fn is_empty(&self) -> bool {
        self.description.is_none() && self.tags.is_empty() && self.attributes.is_empty()
    }

    /// 替换资源描述。
    pub fn set_description(&mut self, description: Option<String>) -> Result<(), ResourceError> {
        self.description = normalize_optional_metadata_text(
            "metadata.description",
            description,
            MAX_METADATA_DESCRIPTION_LEN,
        )?;
        Ok(())
    }

    /// 追加标签。
    pub fn add_tag(&mut self, tag: impl Into<String>) -> Result<(), ResourceError> {
        let tag = normalize_required_text("metadata.tag", &tag.into(), MAX_METADATA_TAG_LEN)?;

        if self.tags.len() >= MAX_METADATA_TAGS && !self.tags.contains(&tag) {
            return Err(ResourceError::TooLong {
                field: "metadata.tags",
                max: MAX_METADATA_TAGS,
            });
        }

        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }

        Ok(())
    }

    /// 写入扩展属性。
    pub fn insert_attribute(
        &mut self,
        key: impl Into<String>,
        value: Value,
    ) -> Result<Option<Value>, ResourceError> {
        let key = normalize_required_text(
            "metadata.attributes.key",
            &key.into(),
            MAX_METADATA_ATTRIBUTE_KEY_LEN,
        )?;

        Ok(self.attributes.insert(key, value))
    }

    fn validate(self) -> Result<Self, ResourceError> {
        if self.schema_version != RESOURCE_METADATA_SCHEMA_VERSION {
            return Err(ResourceError::InvalidFormat {
                field: "metadata.schema_version",
                reason: "unsupported resource metadata schema version",
            });
        }

        Self::new(self.description, self.tags, self.attributes)
    }
}

impl Default for ResourceMetadata {
    /// 提供默认实现，初始为空元数据。
    fn default() -> Self {
        Self {
            schema_version: RESOURCE_METADATA_SCHEMA_VERSION,
            description: None,
            tags: Vec::new(),
            attributes: BTreeMap::new(),
        }
    }
}

/// 资源元数据构建器。
#[derive(Debug, Clone, Default)]
pub struct ResourceMetadataBuilder {
    /// 资源描述。
    description: Option<String>,
    /// 资源标签。
    tags: Vec<String>,
    /// 资源扩展属性。
    attributes: BTreeMap<String, Value>,
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

    /// 写入一个扩展属性。
    pub fn with_attribute(mut self, key: impl Into<String>, value: Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    /// 批量写入扩展属性。
    pub fn with_attributes<T, I>(mut self, attributes: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = (T, Value)>,
    {
        self.attributes.extend(
            attributes
                .into_iter()
                .map(|(key, value)| (key.into(), value)),
        );
        self
    }

    /// 完成构建并执行元数据校验。
    pub fn build(self) -> Result<ResourceMetadata, ResourceError> {
        ResourceMetadata::new(self.description, self.tags, self.attributes)
    }
}

fn default_schema_version() -> u32 {
    RESOURCE_METADATA_SCHEMA_VERSION
}

fn is_structured_metadata_object(object: &serde_json::Map<String, Value>) -> bool {
    object.contains_key("schema_version")
        || object.contains_key("description")
        || object.contains_key("tags")
        || object.contains_key("attributes")
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

fn normalize_tags(tags: Vec<String>) -> Result<Vec<String>, ResourceError> {
    if tags.len() > MAX_METADATA_TAGS {
        return Err(ResourceError::TooLong {
            field: "metadata.tags",
            max: MAX_METADATA_TAGS,
        });
    }

    let mut normalized = Vec::with_capacity(tags.len());

    for tag in tags {
        let tag = normalize_required_text("metadata.tag", &tag, MAX_METADATA_TAG_LEN)?;

        if !normalized.contains(&tag) {
            normalized.push(tag);
        }
    }

    Ok(normalized)
}

fn normalize_attributes(
    attributes: BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, ResourceError> {
    let mut normalized = BTreeMap::new();

    for (key, value) in attributes {
        let key = normalize_required_text(
            "metadata.attributes.key",
            &key,
            MAX_METADATA_ATTRIBUTE_KEY_LEN,
        )?;
        normalized.insert(key, value);
    }

    Ok(normalized)
}

fn parse_schema_version(value: Value) -> Result<u32, ResourceError> {
    let Some(version) = value.as_u64() else {
        return Err(ResourceError::InvalidFormat {
            field: "metadata.schema_version",
            reason: "schema version must be an unsigned integer",
        });
    };

    u32::try_from(version).map_err(|_| ResourceError::TooLong {
        field: "metadata.schema_version",
        max: u32::MAX as usize,
    })
}

fn parse_description(value: Value) -> Result<Option<String>, ResourceError> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        _ => Err(ResourceError::InvalidFormat {
            field: "metadata.description",
            reason: "description must be a string",
        }),
    }
}

fn parse_tags(value: Value) -> Result<Vec<String>, ResourceError> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(tags) => tags
            .into_iter()
            .map(|tag| match tag {
                Value::String(tag) => Ok(tag),
                _ => Err(ResourceError::InvalidFormat {
                    field: "metadata.tags",
                    reason: "tags must be an array of strings",
                }),
            })
            .collect(),
        _ => Err(ResourceError::InvalidFormat {
            field: "metadata.tags",
            reason: "tags must be an array of strings",
        }),
    }
}

fn parse_attributes(value: Value) -> Result<BTreeMap<String, Value>, ResourceError> {
    match value {
        Value::Null => Ok(BTreeMap::new()),
        Value::Object(attributes) => Ok(attributes.into_iter().collect()),
        _ => Err(ResourceError::InvalidFormat {
            field: "metadata.attributes",
            reason: "attributes must be a JSON object",
        }),
    }
}
