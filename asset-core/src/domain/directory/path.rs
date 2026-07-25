use super::{MAX_DIRECTORY_SEGMENT_LEN, validate_required_text_exact};
use crate::error::DirectoryError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

const MAX_DIRECTORY_PATH_LEN: usize = 1024;
/// Asset Hub 内部存储目录名，不属于用户可见资源目录空间。
pub const INTERNAL_STORAGE_DIRECTORY_NAME: &str = ".asset-hub";

/// 用户可见目录的规范化路径值对象。
///
/// 路径用于 HTTP、对象存储和查询投影，不承担目录身份。目录身份由
/// [`super::DirectoryId`] 表示，因此目录移动和重命名不会让引用失效。
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirectoryPath {
    path: String,
}

impl DirectoryPath {
    /// 返回根目录。
    pub fn root() -> Self {
        Self::default()
    }

    /// 从完整目录路径创建值对象，并执行规范化和领域校验。
    pub fn from_path(path: impl Into<String>) -> Result<Self, DirectoryError> {
        normalize_path(path.into()).map(|path| Self { path })
    }

    /// 在父目录下创建一个直接子目录。
    pub fn child(&self, name: impl Into<String>) -> Result<Self, DirectoryError> {
        let name = name.into();
        let name = name.as_str();
        if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.path",
                reason: "directory name must be a single path segment",
            });
        }
        let name = validate_required_text_exact("directory.path", name, MAX_DIRECTORY_SEGMENT_LEN)?;
        let path = if self.is_root() {
            name
        } else {
            format!("{}/{name}", self.path())
        };
        Self::from_path(path)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn parent_path(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(parent, _)| parent)
    }

    pub fn name(&self) -> &str {
        self.path
            .rsplit_once('/')
            .map_or(self.path.as_str(), |(_, name)| name)
    }

    pub fn is_root(&self) -> bool {
        self.path.is_empty()
    }

    /// 判断当前目录是否为目标目录本身或其祖先目录。
    pub fn contains(&self, target: &Self) -> bool {
        self.is_root()
            || self == target
            || target
                .path()
                .strip_prefix(self.path())
                .is_some_and(|suffix| suffix.starts_with('/'))
    }
}

impl FromStr for DirectoryPath {
    type Err = DirectoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_path(value)
    }
}

impl TryFrom<String> for DirectoryPath {
    type Error = DirectoryError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_path(value)
    }
}

impl AsRef<str> for DirectoryPath {
    fn as_ref(&self) -> &str {
        self.path()
    }
}

impl fmt::Display for DirectoryPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.path())
    }
}

impl Serialize for DirectoryPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.path())
    }
}

impl<'de> Deserialize<'de> for DirectoryPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = String::deserialize(deserializer)?;
        Self::from_path(path).map_err(serde::de::Error::custom)
    }
}

fn normalize_path(value: String) -> Result<String, DirectoryError> {
    let value = value.replace('\\', "/");
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.starts_with('/') {
        return Err(DirectoryError::InvalidFormat {
            field: "directory.path",
            reason: "absolute paths are not allowed",
        });
    }

    let mut parts = Vec::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.path",
                reason: "parent path segments are not allowed",
            });
        }
        parts.push(validate_required_text_exact(
            "directory.path",
            part,
            MAX_DIRECTORY_SEGMENT_LEN,
        )?);
    }

    let path = parts.join("/");
    if parts
        .first()
        .is_some_and(|part| part == INTERNAL_STORAGE_DIRECTORY_NAME)
    {
        return Err(DirectoryError::InvalidFormat {
            field: "directory.path",
            reason: "the .asset-hub directory is reserved for internal storage",
        });
    }
    if path.chars().count() > MAX_DIRECTORY_PATH_LEN {
        return Err(DirectoryError::TooLong {
            field: "directory.path",
            max: MAX_DIRECTORY_PATH_LEN,
        });
    }
    Ok(path)
}
