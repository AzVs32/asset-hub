use crate::error::DirectoryError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 目录类型允许的最大字符数。
const MAX_DIRECTORY_KIND_LEN: usize = 256;

/// 目录类型值对象。插件可以贡献自己的 `namespace:name` 类型。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DirectoryKind(String);

impl DirectoryKind {
    /// 未指定具体目录类型时使用的默认目录类型。
    pub const DEFAULT: &'static str = "core:directory";

    /// 创建、规范化并校验目录类型。
    pub fn try_new(value: impl Into<String>) -> Result<Self, DirectoryError> {
        normalize_directory_kind(value.into()).map(Self)
    }

    /// 获取内部原始字符串的只读借用（`&str`）。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Default for DirectoryKind {
    fn default() -> Self {
        Self::try_new(Self::DEFAULT).expect("core:directory must be a valid directory kind")
    }
}

impl TryFrom<String> for DirectoryKind {
    type Error = DirectoryError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<DirectoryKind> for String {
    fn from(value: DirectoryKind) -> Self {
        value.0
    }
}

impl fmt::Display for DirectoryKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn normalize_directory_kind(value: String) -> Result<String, DirectoryError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(DirectoryError::Blank {
            field: "directory.kind",
        });
    }
    if value.chars().count() > MAX_DIRECTORY_KIND_LEN {
        return Err(DirectoryError::TooLong {
            field: "directory.kind",
            max: MAX_DIRECTORY_KIND_LEN,
        });
    }
    let value = value.to_ascii_lowercase();
    let Some((namespace, name)) = value.split_once(':') else {
        return Err(DirectoryError::InvalidFormat {
            field: "directory.kind",
            reason: "directory kind must use namespace:name format",
        });
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_')
                    || character == '.'
            })
    };
    if !valid(namespace) || !valid(name) {
        return Err(DirectoryError::InvalidFormat {
            field: "directory.kind",
            reason: "directory kind contains invalid characters",
        });
    }
    Ok(value)
}
