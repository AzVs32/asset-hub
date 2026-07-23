use crate::error::DirectoryError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::{fmt, str::FromStr};

const MAX_DIRECTORY_PATH_LEN: usize = 1024;
const MAX_DIRECTORY_SEGMENT_LEN: usize = 255;
/// Asset Hub 内部存储目录名，不属于用户可见资源目录空间。
pub const INTERNAL_STORAGE_DIRECTORY_NAME: &str = ".asset-hub";
pub const CORE_DIRECTORY_KIND: &str = "core:directory";

crate::gen_id_uuid_v7!(DirectoryId);

/// 目录类型标识。插件可以贡献自己的 `namespace:name` 类型。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DirectoryKind(String);

impl DirectoryKind {
    pub fn try_new(value: impl Into<String>) -> Result<Self, DirectoryError> {
        normalize_directory_kind(value.into()).map(Self)
    }

    pub fn core() -> Self {
        Self(CORE_DIRECTORY_KIND.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
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

impl DirectoryId {
    /// 全局根目录使用固定标识，保证不同进程和首次建库得到相同的根节点。
    pub fn root() -> Self {
        Self::from_uuid(uuid::Uuid::nil())
    }

    pub fn is_root(self) -> bool {
        self == Self::root()
    }
}

/// 用户可见目录的规范化路径值对象。
///
/// 路径用于 HTTP、对象存储和查询投影，不承担目录身份。目录身份由 [`DirectoryId`]
/// 表示，因此目录移动和重命名不会让引用失效。
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

/// 对目录聚合的稳定引用，并携带当前路径查询投影。
///
/// `id` 是领域关系中的事实；`path` 只用于本次读取快照中的展示和存储定位。Repository
/// 只持久化 `id`，读取时通过目录树重建路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryRef {
    id: DirectoryId,
    path: DirectoryPath,
}

impl DirectoryRef {
    pub fn new(id: DirectoryId, path: DirectoryPath) -> Self {
        Self { id, path }
    }

    pub fn root() -> Self {
        Self::new(DirectoryId::root(), DirectoryPath::root())
    }

    pub fn id(&self) -> DirectoryId {
        self.id
    }

    pub fn path(&self) -> &DirectoryPath {
        &self.path
    }
}

/// 独立的目录聚合根。
///
/// 聚合只保存自身及直接父目录标识，不加载子树。完整路径、祖先链和后代列表属于查询
/// 模型，由 DirectoryRepository 使用递归查询组合。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Directory {
    id: DirectoryId,
    parent_id: Option<DirectoryId>,
    name: String,
    kind: DirectoryKind,
    metadata: Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectorySnapshot {
    pub id: DirectoryId,
    pub parent_id: Option<DirectoryId>,
    pub name: String,
    pub kind: DirectoryKind,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Directory {
    pub fn new(parent_id: DirectoryId, name: impl Into<String>) -> Result<Self, DirectoryError> {
        let now = Utc::now();
        Self::rehydrate(DirectorySnapshot {
            id: DirectoryId::new(),
            parent_id: Some(parent_id),
            name: name.into(),
            kind: DirectoryKind::core(),
            metadata: Value::Object(Default::default()),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn root() -> Self {
        let now = Utc::now();
        Self {
            id: DirectoryId::root(),
            parent_id: None,
            name: String::new(),
            kind: DirectoryKind::core(),
            metadata: Value::Object(Default::default()),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn rehydrate(snapshot: DirectorySnapshot) -> Result<Self, DirectoryError> {
        if snapshot.id.is_root() {
            if snapshot.parent_id.is_some() || !snapshot.name.is_empty() {
                return Err(DirectoryError::InvalidFormat {
                    field: "directory.root",
                    reason: "root directory cannot have a parent or name",
                });
            }
        } else {
            if snapshot.parent_id.is_none() {
                return Err(DirectoryError::InvalidFormat {
                    field: "directory.parent_id",
                    reason: "non-root directory must have a parent",
                });
            }
            validate_directory_name(&snapshot.name)?;
        }
        if !snapshot.metadata.is_object() {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.metadata",
                reason: "directory metadata must be a JSON object",
            });
        }
        Ok(Self {
            id: snapshot.id,
            parent_id: snapshot.parent_id,
            name: snapshot.name,
            kind: snapshot.kind,
            metadata: snapshot.metadata,
            created_at: snapshot.created_at,
            updated_at: snapshot.updated_at,
        })
    }

    pub fn id(&self) -> DirectoryId {
        self.id
    }

    pub fn parent_id(&self) -> Option<DirectoryId> {
        self.parent_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> &DirectoryKind {
        &self.kind
    }

    pub fn metadata(&self) -> &Value {
        &self.metadata
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub fn rename(&mut self, name: impl Into<String>) -> Result<(), DirectoryError> {
        if self.id.is_root() {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.name",
                reason: "root directory cannot be renamed",
            });
        }
        let name = validate_directory_name(&name.into())?;
        if self.name != name {
            self.name = name;
            self.touch();
        }
        Ok(())
    }

    pub fn move_to(&mut self, parent_id: DirectoryId) -> Result<(), DirectoryError> {
        if self.id.is_root() {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.parent_id",
                reason: "root directory cannot be moved",
            });
        }
        if self.id == parent_id {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.parent_id",
                reason: "directory cannot be its own parent",
            });
        }
        if self.parent_id != Some(parent_id) {
            self.parent_id = Some(parent_id);
            self.touch();
        }
        Ok(())
    }

    pub fn change_kind(&mut self, kind: impl Into<String>) -> Result<(), DirectoryError> {
        let kind = DirectoryKind::try_new(kind)?;
        if self.kind != kind {
            self.kind = kind;
            self.touch();
        }
        Ok(())
    }

    pub fn replace_metadata(&mut self, metadata: Value) -> Result<(), DirectoryError> {
        if !metadata.is_object() {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.metadata",
                reason: "directory metadata must be a JSON object",
            });
        }
        if self.metadata != metadata {
            self.metadata = metadata;
            self.touch();
        }
        Ok(())
    }

    fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

fn validate_directory_name(value: &str) -> Result<String, DirectoryError> {
    if value == "." || value == ".." || value.contains('/') || value.contains('\\') {
        return Err(DirectoryError::InvalidFormat {
            field: "directory.name",
            reason: "directory name must be a single path segment",
        });
    }
    validate_required_text_exact("directory.name", value, MAX_DIRECTORY_SEGMENT_LEN)
}

fn validate_required_text_exact(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, DirectoryError> {
    if value.trim().is_empty() {
        return Err(DirectoryError::Blank { field });
    }
    if value.chars().count() > max {
        return Err(DirectoryError::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(DirectoryError::InvalidFormat {
            field,
            reason: "control characters are not allowed",
        });
    }
    Ok(value.to_owned())
}

fn normalize_directory_kind(value: String) -> Result<String, DirectoryError> {
    let value = value.trim().to_ascii_lowercase();
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn directory_path_is_built_from_parent_and_single_name() {
        let parent = DirectoryPath::from_path("projects").unwrap();
        let directory = parent.child(" images ").unwrap();

        assert_eq!(directory.path(), "projects/ images ");
        assert_eq!(directory.parent_path(), "projects");
        assert_eq!(directory.name(), " images ");
        assert!(parent.child("../secret").is_err());
    }

    #[test]
    fn directory_path_supports_root_and_normalizes_segments() {
        assert!(DirectoryPath::from_path("").unwrap().is_root());
        assert!(DirectoryPath::from_path("  ").is_err());
        assert_eq!(
            DirectoryPath::from_path("projects\\images/./raw")
                .unwrap()
                .path(),
            "projects/images/raw"
        );
    }

    #[test]
    fn directory_path_rejects_internal_storage_namespace() {
        for path in [".asset-hub", ".asset-hub/trash"] {
            assert!(matches!(
                DirectoryPath::from_path(path),
                Err(DirectoryError::InvalidFormat {
                    field: "directory.path",
                    ..
                })
            ));
        }
    }

    #[test]
    fn directory_path_serde_uses_the_path_representation() {
        let directory = DirectoryPath::from_path("projects/images").unwrap();
        let json = serde_json::to_string(&directory).unwrap();

        assert_eq!(json, "\"projects/images\"");
        assert_eq!(
            serde_json::from_str::<DirectoryPath>(&json).unwrap(),
            directory
        );
    }

    #[test]
    fn directory_path_contains_obeys_segment_boundaries() {
        let root = DirectoryPath::root();
        let home = DirectoryPath::from_path("users/alice").unwrap();
        let child = DirectoryPath::from_path("users/alice/photos").unwrap();
        let sibling = DirectoryPath::from_path("users/alice2").unwrap();

        assert!(root.contains(&home));
        assert!(home.contains(&home));
        assert!(home.contains(&child));
        assert!(!home.contains(&sibling));
    }

    #[test]
    fn directory_is_an_independent_aggregate_with_a_stable_identity() {
        let parent = DirectoryId::new();
        let mut directory = Directory::new(parent, "Games").unwrap();
        let id = directory.id();

        directory.rename("Library").unwrap();
        directory.move_to(DirectoryId::new()).unwrap();
        directory.change_kind("azvs.game:library").unwrap();
        directory
            .replace_metadata(json!({"platform": "windows"}))
            .unwrap();

        assert_eq!(directory.id(), id);
        assert_eq!(directory.name(), "Library");
        assert_eq!(directory.kind().as_str(), "azvs.game:library");
        assert_eq!(directory.metadata(), &json!({"platform": "windows"}));
    }

    #[test]
    fn root_directory_has_fixed_identity_and_cannot_be_mutated_as_a_child() {
        let mut root = Directory::root();

        assert_eq!(root.id(), DirectoryId::root());
        assert!(root.parent_id().is_none());
        assert!(root.rename("root").is_err());
        assert!(root.move_to(DirectoryId::new()).is_err());
    }

    #[test]
    fn directory_rejects_self_parent_and_non_object_metadata() {
        let mut directory = Directory::new(DirectoryId::root(), "Games").unwrap();

        assert!(directory.move_to(directory.id()).is_err());
        assert!(directory.replace_metadata(json!(["invalid"])).is_err());
    }
}
