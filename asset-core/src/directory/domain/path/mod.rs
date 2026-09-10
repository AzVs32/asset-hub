use super::Directory;
use crate::{error::DirectoryError, storage::RESERVED_BLOB_STORAGE_PREFIX};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

const MAX_DIRECTORY_PATH_LEN: usize = 1024;
/// 用户可见目录的规范化路径值对象。
///
/// 路径用于 HTTP、对象存储和查询投影，不承担目录身份。目录身份由
/// [`super::DirectoryId`] 表示，因此目录移动和重命名不会让引用失效。
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirectoryPath {
    path: String,
}

impl DirectoryPath {
    /// 返回全局根目录路径，即空字符串。
    pub fn root() -> Self {
        Self {
            path: String::new(),
        }
    }

    /// 从完整目录路径创建值对象，并执行规范化和领域校验。
    pub fn from_path(path: impl Into<String>) -> Result<Self, DirectoryError> {
        let path = Self::normalize(&path.into());
        Self::validate(&path)?;
        Ok(Self { path })
    }

    /// 在父目录下创建一个直接子目录。
    pub fn child(&self, name: impl Into<String>) -> Result<Self, DirectoryError> {
        let name = name.into();
        Directory::validate_name(&name)?;
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

impl DirectoryPath {
    /// 将目录路径转换为统一的规范表示。
    ///
    /// 规范化仅改变路径的表示形式，不判断其是否满足
    /// [`DirectoryPath`] 的领域约束：
    ///
    /// - `\` 转换为 `/`；
    /// - 移除重复的路径分隔符；
    /// - 移除 `.` 路径段；
    /// - 移除尾部路径分隔符。
    ///
    /// 不解析 `..`，也不会将绝对路径转换为相对路径。
    fn normalize(value: &str) -> String {
        let is_absolute = value.starts_with('/') || value.starts_with('\\');
        let mut path = String::with_capacity(value.len());
        if is_absolute {
            path.push('/');
        }

        for part in value
            .split(['/', '\\'])
            .filter(|part| !part.is_empty() && *part != ".")
        {
            if !path.is_empty() && !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(part);
        }
        path
    }

    /// 校验规范化后的目录路径是否满足 [`DirectoryPath`] 的领域约束。
    ///
    /// - 路径必须是相对路径；
    /// - 不允许 `..` 路径段；
    /// - 每个路径段必须满足目录名称规则；
    /// - 根级 `.asset-hub` 命名空间保留给内部存储；
    /// - 完整路径长度不能超过上限。
    fn validate(value: &str) -> Result<(), DirectoryError> {
        if value.is_empty() {
            return Ok(());
        }
        if value.starts_with('/') {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.path",
                reason: "absolute paths are not allowed",
            });
        }

        let mut parts = value.split('/');
        let first = parts
            .next()
            .expect("a non-empty normalized path must contain a segment");
        Directory::validate_name(first)?;
        for part in parts {
            Directory::validate_name(part)?;
        }

        if first == RESERVED_BLOB_STORAGE_PREFIX {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.path",
                reason: "the .asset-hub directory is reserved for internal storage",
            });
        }
        if value.chars().count() > MAX_DIRECTORY_PATH_LEN {
            return Err(DirectoryError::TooLong {
                field: "directory.path",
                max: MAX_DIRECTORY_PATH_LEN,
            });
        }
        Ok(())
    }
}
