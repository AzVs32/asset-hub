use crate::ResourceError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

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
    /// 返回跨 HTTP、持久化和插件边界使用的规范文本值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    /// 判断状态是否为活跃。
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// 判断状态是否为归档。
    pub fn is_archived(&self) -> bool {
        matches!(self, Self::Archived)
    }
}

impl fmt::Display for ResourceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ResourceStatus {
    type Err = ResourceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            _ => Err(ResourceError::InvalidFormat {
                field: "resource.status",
                reason: "expected `active` or `archived`",
            }),
        }
    }
}

impl Default for ResourceStatus {
    /// 默认值： "Active"。
    fn default() -> Self {
        Self::Active
    }
}
