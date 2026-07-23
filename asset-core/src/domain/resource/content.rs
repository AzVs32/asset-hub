use super::{ResourceDirectory, normalize_required_text, validate_required_text_exact};
use crate::error::ResourceError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// 内容描述类文本允许的最大字符数。
const MAX_CONTENT_TEXT_LEN: usize = 255;
/// 存储键允许的最大字符数。
const MAX_STORAGE_KEY_LEN: usize = 1024;

// ==================================================
// 资源内容
// ==================================================

/// 资源内容引用。
///
/// 内容本体由外部存储系统管理，本结构保存内容属性，以及最近一次成功协调时观察到的
/// 物理对象修改时间。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceContent {
    /// 内容字节大小。
    size: u64,
    /// 内容 MIME 类型。
    mime_type: Option<String>,
    /// 根据内容本体计算得到的唯一校验和。
    checksum: Checksum,
    /// 最近一次成功协调时观察到的物理存储修改时间。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified_at: Option<DateTime<Utc>>,
}

impl ResourceContent {
    /// 创建内容引用构建器。
    pub fn builder(size: u64, checksum: Checksum) -> ResourceContentBuilder {
        ResourceContentBuilder::new(size, checksum)
    }

    /// 返回内容字节大小。
    pub fn size(&self) -> u64 {
        self.size
    }

    /// 返回内容 MIME 类型。
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }

    /// 返回根据内容本体计算得到的校验和。
    pub fn checksum(&self) -> &Checksum {
        &self.checksum
    }

    /// 返回最近一次成功协调时观察到的物理存储修改时间。
    pub fn modified_at(&self) -> Option<DateTime<Utc>> {
        self.modified_at
    }
}

/// 资源内容引用构建器。
#[derive(Debug, Clone)]
pub struct ResourceContentBuilder {
    /// 内容字节大小。
    size: u64,
    /// 内容 MIME 类型。
    mime_type: Option<String>,
    /// 根据内容本体计算得到的唯一校验和。
    checksum: Checksum,
    /// 最近一次成功协调时观察到的物理存储修改时间。
    modified_at: Option<DateTime<Utc>>,
}

impl ResourceContentBuilder {
    /// 创建资源内容引用构建器。
    pub fn new(size: u64, checksum: Checksum) -> Self {
        Self {
            size,
            mime_type: None,
            checksum,
            modified_at: None,
        }
    }

    /// 设置内容 MIME 类型。
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// 设置最近一次成功协调时观察到的物理存储修改时间。
    pub fn with_modified_at(mut self, modified_at: DateTime<Utc>) -> Self {
        self.modified_at = Some(modified_at);
        self
    }

    /// 完成构建并执行领域校验。
    pub fn build(self) -> Result<ResourceContent, ResourceError> {
        let mime_type = self
            .mime_type
            .map(|mime_type| {
                normalize_required_text("content.mime_type", &mime_type, MAX_CONTENT_TEXT_LEN)
            })
            .transpose()?;

        Ok(ResourceContent {
            size: self.size,
            mime_type,
            checksum: self.checksum,
            modified_at: self.modified_at,
        })
    }
}

/// 存储键值对象。
///
/// 存储键是面向存储适配器的相对路径或对象键，不允许使用绝对路径和父级路径片段。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey(String);

impl StorageKey {
    /// 从资源逻辑目录和文件名生成唯一的对象存储键。
    pub fn from_resource_path(
        directory: &ResourceDirectory,
        name: &str,
    ) -> Result<Self, ResourceError> {
        let value = if directory.is_root() {
            name.to_owned()
        } else {
            format!("{}/{name}", directory.path())
        };
        Self::new(value)
    }

    /// 创建并校验存储键。
    pub fn new(value: impl Into<String>) -> Result<Self, ResourceError> {
        let value =
            validate_required_text_exact("storage.key", &value.into(), MAX_STORAGE_KEY_LEN)?;

        if value.starts_with('/') {
            return Err(ResourceError::InvalidFormat {
                field: "storage.key",
                reason: "absolute paths are not allowed",
            });
        }

        if value.split('/').any(|part| part == "..") {
            return Err(ResourceError::InvalidFormat {
                field: "storage.key",
                reason: "parent path segments are not allowed",
            });
        }

        Ok(Self(value))
    }

    /// 返回存储键原始字符串。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for StorageKey {
    type Err = ResourceError;

    /// 从字符串解析存储键。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for StorageKey {
    type Error = ResourceError;

    /// 从 `String` 创建并校验存储键。
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for StorageKey {
    type Error = ResourceError;

    /// 从字符串切片创建并校验存储键。
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// 内容校验和值对象。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checksum {
    /// 校验和算法类型。
    kind: ChecksumKind,
    /// 校验和值。
    value: String,
}

impl Checksum {
    /// 创建并校验指定类型的校验和。
    pub fn new(kind: ChecksumKind, value: impl Into<String>) -> Result<Self, ResourceError> {
        let value = value.into().trim().to_string();

        match kind {
            ChecksumKind::Sha256 => validate_sha256(&value)?,
        }

        Ok(Self { kind, value })
    }

    /// 创建 SHA-256 校验和。
    pub fn sha256(value: impl Into<String>) -> Result<Self, ResourceError> {
        Self::new(ChecksumKind::Sha256, value)
    }

    /// 返回校验和算法类型。
    pub fn kind(&self) -> ChecksumKind {
        self.kind
    }

    /// 返回校验和值。
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// 内容校验和算法类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumKind {
    /// SHA-256 校验和。
    Sha256,
}

impl ChecksumKind {
    /// 返回跨 HTTP、持久化和插件边界使用的规范文本值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
        }
    }
}

impl fmt::Display for ChecksumKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ChecksumKind {
    type Err = ResourceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "sha256" => Ok(Self::Sha256),
            _ => Err(ResourceError::InvalidFormat {
                field: "checksum.kind",
                reason: "unsupported checksum algorithm",
            }),
        }
    }
}

/// 校验 SHA-256 字符串格式。
fn validate_sha256(value: &str) -> Result<(), ResourceError> {
    if value.len() != 64 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(ResourceError::InvalidFormat {
            field: "checksum.sha256",
            reason: "expected 64 hexadecimal characters",
        });
    }

    Ok(())
}
