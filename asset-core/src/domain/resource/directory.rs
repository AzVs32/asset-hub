use super::validate_required_text_exact;
use crate::error::ResourceError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

const MAX_RESOURCE_DIRECTORY_LEN: usize = 1024;
const MAX_DIRECTORY_SEGMENT_LEN: usize = 255;
/// Asset Hub 内部存储目录名，不属于用户可见资源目录空间。
pub const INTERNAL_STORAGE_DIRECTORY_NAME: &str = ".asset-hub";

/// 用户可见资源目录的规范化路径值对象。
///
/// 根目录由空路径隐式表示；其余目录必须同时存在于目录仓储与实际存储中。
/// Asset Hub 内部使用的 `.asset-hub` 命名空间不属于本模型。
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceDirectory {
    path: String,
    parent_path: String,
    name: String,
}

impl ResourceDirectory {
    /// 返回根目录。
    pub fn root() -> Self {
        Self::default()
    }

    /// 从完整目录路径创建值对象，并执行规范化和领域校验。
    pub fn from_path(path: impl Into<String>) -> Result<Self, ResourceError> {
        let path = normalize_path(path.into())?;
        if path.is_empty() {
            return Ok(Self::root());
        }

        let (parent_path, name) = path.rsplit_once('/').map_or_else(
            || (String::new(), path.clone()),
            |(parent, name)| (parent.to_owned(), name.to_owned()),
        );

        Ok(Self {
            path,
            parent_path,
            name,
        })
    }

    /// 在父目录下创建一个直接子目录。
    pub fn child(&self, name: impl Into<String>) -> Result<Self, ResourceError> {
        let name = name.into();
        let name = name.as_str();
        if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
            return Err(ResourceError::InvalidFormat {
                field: "resource.directory",
                reason: "directory name must be a single path segment",
            });
        }
        let name =
            validate_required_text_exact("resource.directory", name, MAX_DIRECTORY_SEGMENT_LEN)?;
        let path = if self.is_root() {
            name
        } else {
            format!("{}/{name}", self.path())
        };
        Self::from_path(path)
    }

    /// 从持久化字段还原目录，并验证派生字段没有相互矛盾。
    pub fn rehydrate(
        path: String,
        parent_path: String,
        name: String,
    ) -> Result<Self, ResourceError> {
        let directory = Self::from_path(path.clone())?;
        if directory.path != path || directory.parent_path != parent_path || directory.name != name
        {
            return Err(ResourceError::InvalidFormat {
                field: "resource.directory",
                reason: "persisted directory fields are inconsistent",
            });
        }
        Ok(directory)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn parent_path(&self) -> &str {
        &self.parent_path
    }

    pub fn name(&self) -> &str {
        &self.name
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

impl FromStr for ResourceDirectory {
    type Err = ResourceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_path(value)
    }
}

impl TryFrom<String> for ResourceDirectory {
    type Error = ResourceError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_path(value)
    }
}

impl AsRef<str> for ResourceDirectory {
    fn as_ref(&self) -> &str {
        self.path()
    }
}

impl fmt::Display for ResourceDirectory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.path())
    }
}

impl Serialize for ResourceDirectory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.path())
    }
}

impl<'de> Deserialize<'de> for ResourceDirectory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = String::deserialize(deserializer)?;
        Self::from_path(path).map_err(serde::de::Error::custom)
    }
}

fn normalize_path(value: String) -> Result<String, ResourceError> {
    let value = value.replace('\\', "/");
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.starts_with('/') {
        return Err(ResourceError::InvalidFormat {
            field: "resource.directory",
            reason: "absolute paths are not allowed",
        });
    }

    let mut parts = Vec::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(ResourceError::InvalidFormat {
                field: "resource.directory",
                reason: "parent path segments are not allowed",
            });
        }
        parts.push(validate_required_text_exact(
            "resource.directory",
            part,
            MAX_DIRECTORY_SEGMENT_LEN,
        )?);
    }

    let path = parts.join("/");
    if parts
        .first()
        .is_some_and(|part| part == INTERNAL_STORAGE_DIRECTORY_NAME)
    {
        return Err(ResourceError::InvalidFormat {
            field: "resource.directory",
            reason: "the .asset-hub directory is reserved for internal storage",
        });
    }
    if path.chars().count() > MAX_RESOURCE_DIRECTORY_LEN {
        return Err(ResourceError::TooLong {
            field: "resource.directory",
            max: MAX_RESOURCE_DIRECTORY_LEN,
        });
    }
    Ok(path)
}
