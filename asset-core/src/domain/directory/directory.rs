use super::{DirectoryKind, MAX_DIRECTORY_SEGMENT_LEN, validate_required_text_exact};
use crate::error::DirectoryError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

crate::gen_id_uuid_v7!(DirectoryId);

impl DirectoryId {
    /// 全局根目录使用固定标识，保证不同进程和首次建库得到相同的根节点。
    pub fn root() -> Self {
        Self::from_uuid(uuid::Uuid::nil())
    }

    pub fn is_root(self) -> bool {
        self == Self::root()
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
            kind: DirectoryKind::default(),
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
            kind: DirectoryKind::default(),
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
