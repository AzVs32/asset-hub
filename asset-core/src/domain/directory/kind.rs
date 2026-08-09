use crate::domain::{KindId, KindIdError};
use serde::{Deserialize, Serialize};
use std::fmt;

/// 目录类型值对象。插件可以贡献自己的 `namespace:name` 类型。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DirectoryKind(KindId);

impl DirectoryKind {
    /// 未指定具体目录类型时使用的默认目录类型。
    pub const DEFAULT: &'static str = "core:directory";

    /// 创建、规范化并校验目录类型。
    pub fn try_new(value: impl Into<String>) -> Result<Self, KindIdError> {
        KindId::new(value).map(Self)
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
    type Error = KindIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<DirectoryKind> for String {
    fn from(value: DirectoryKind) -> Self {
        value.0.into()
    }
}

impl fmt::Display for DirectoryKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl AsRef<str> for DirectoryKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::str::FromStr for DirectoryKind {
    type Err = KindIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_new(value)
    }
}
