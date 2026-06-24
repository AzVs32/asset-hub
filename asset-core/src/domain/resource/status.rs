use serde::{Deserialize, Serialize};

// ==================================================
// 资源状态
// ==================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceStatus {
    /// 资源正常可用。
    Active,
    /// 资源已归档，但未删除。
    Archived,
}

impl ResourceStatus {
    /// 判断状态是否为活跃。
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// 判断状态是否为归档。
    pub fn is_archived(&self) -> bool {
        matches!(self, Self::Archived)
    }
}

impl Default for ResourceStatus {
    /// 默认值： "Active"。
    fn default() -> Self {
        Self::Active
    }
}
