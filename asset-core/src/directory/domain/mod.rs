//! 目录聚合及其类型、路径值对象。

mod path;

use crate::error::DirectoryError;
use chrono::{DateTime, Utc};
pub use path::DirectoryPath;
use serde::Serialize;

// 允许的最大 directory.name 长度
const MAX_DIRECTORY_NAME_LEN: usize = 255;

// 约定：Slot0 为 root 目录的固定 ID
crate::gen_id_uuid_v7!(DirectoryId, DirectoryIdSlot);

/// 独立的目录聚合根。
///
/// 聚合只保存自身及直接父目录标识，不加载子树。完整路径、祖先链和后代列表属于查询
/// 模型，由可重建的 DirectoryIndex 查询投影组合。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Directory {
    id: DirectoryId,
    parent_id: Option<DirectoryId>,
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revision: u64,
}

impl Directory {
    pub fn new(parent_id: DirectoryId, name: impl Into<String>) -> Result<Self, DirectoryError> {
        let name = name.into();
        Self::validate_name(&name)?;
        let now = Utc::now();
        Ok(Self {
            id: DirectoryId::new(),
            parent_id: Some(parent_id),
            name,
            created_at: now,
            updated_at: now,
            revision: 1,
        })
    }

    pub fn root() -> Self {
        let now = Utc::now();
        Self {
            id: DirectoryId::from_slot(DirectoryIdSlot::Slot0),
            parent_id: None,
            name: String::new(),
            created_at: now,
            updated_at: now,
            revision: 1,
        }
    }

    /// 判断当前 directory 是否为 root
    pub fn is_root(&self) -> bool {
        self.id.slot() == Some(DirectoryIdSlot::Slot0)
    }

    /// 给 directory 重命名
    pub fn rename(&mut self, name: impl Into<String>) -> Result<(), DirectoryError> {
        if self.is_root() {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.name",
                reason: "root directory cannot be renamed",
            });
        }
        let name = name.into();
        Self::validate_name(&name)?;
        if self.name != name {
            self.name = name;
            self.touch();
        }
        Ok(())
    }

    /// 移动 directory 的位置
    pub fn move_to(&mut self, parent_id: DirectoryId) -> Result<(), DirectoryError> {
        if self.is_root() {
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

    /// 从持久化适配器已解析的完整状态还原目录聚合。
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: DirectoryId,
        parent_id: Option<DirectoryId>,
        name: String,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        revision: u64,
    ) -> Result<Self, DirectoryError> {
        let directory = Self {
            id,
            parent_id,
            name,
            created_at,
            updated_at,
            revision,
        };
        if directory.is_root() {
            if directory.parent_id.is_some() || !directory.name.is_empty() {
                return Err(DirectoryError::InvalidFormat {
                    field: "directory.root",
                    reason: "root directory cannot have a parent or name",
                });
            }
        } else {
            if directory.parent_id.is_none() {
                return Err(DirectoryError::InvalidFormat {
                    field: "directory.parent_id",
                    reason: "non-root directory must have a parent",
                });
            }
            if directory.parent_id == Some(directory.id) {
                return Err(DirectoryError::InvalidFormat {
                    field: "directory.parent_id",
                    reason: "directory cannot be its own parent",
                });
            }
            Self::validate_name(&directory.name)?;
        }
        if directory.updated_at < directory.created_at {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.updated_at",
                reason: "updated timestamp cannot precede creation",
            });
        }
        if directory.revision == 0 {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.revision",
                reason: "directory revision must be greater than zero",
            });
        }
        Ok(directory)
    }
}

impl Directory {
    pub fn id(&self) -> DirectoryId {
        self.id
    }

    pub fn parent_id(&self) -> Option<DirectoryId> {
        self.parent_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn validate_name(value: &str) -> Result<(), DirectoryError> {
        // 文件命名拦截：不能是单独的(.)和(..)，不能嵌套(/)和(\)。
        if value == "." || value == ".." || value.contains('/') || value.contains('\\') {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.name",
                reason: "directory name must be a single path segment",
            });
        }
        Self::validate_required_text(value)
    }

    fn validate_required_text(value: &str) -> Result<(), DirectoryError> {
        // 拒绝空字符
        if value.trim().is_empty() {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.name",
                reason: "cannot be blank",
            });
        }
        // Unicode 安全的长度计算
        if value.chars().count() > MAX_DIRECTORY_NAME_LEN {
            return Err(DirectoryError::TooLong {
                field: "directory.name",
                max: MAX_DIRECTORY_NAME_LEN,
            });
        }
        // 控制字符 黑名单
        if value.chars().any(char::is_control) {
            return Err(DirectoryError::InvalidFormat {
                field: "directory.name",
                reason: "control characters are not allowed",
            });
        }
        Ok(())
    }

    /// 当 directory 出现变更，推进“版本号” 及 更新“最后修改时间”。
    fn touch(&mut self) {
        self.updated_at = Utc::now();
        self.revision = self
            .revision
            .checked_add(1)
            .expect("directory revision should not exhaust u64");
    }
}

#[cfg(test)]
mod tests;
