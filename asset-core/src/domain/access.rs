use super::{ResourceDirectory, UserId};
use serde::{Deserialize, Serialize};

/// 已通过外部入口认证的访问主体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessContext {
    user_id: UserId,
    administrator: bool,
}

impl AccessContext {
    pub fn member(user_id: UserId) -> Self {
        Self {
            user_id,
            administrator: false,
        }
    }
    pub fn administrator(user_id: UserId) -> Self {
        Self {
            user_id,
            administrator: true,
        }
    }
    pub fn user_id(&self) -> UserId {
        self.user_id
    }
    pub fn is_administrator(&self) -> bool {
        self.administrator
    }
}

/// 目录中的资源访问权限。授权管理始终由管理员角色控制，不属于该枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryPermission {
    Read,
    Write,
    Full,
}

impl DirectoryPermission {
    /// 显式判断当前权限是否包含所需能力，避免依赖枚举声明顺序。
    pub const fn allows(self, required: Self) -> bool {
        match self {
            Self::Read => matches!(required, Self::Read),
            Self::Write => matches!(required, Self::Read | Self::Write),
            Self::Full => true,
        }
    }

    /// 返回两项权限中能力更强的一项。
    pub const fn stronger(self, other: Self) -> Self {
        if self.allows(other) { self } else { other }
    }
}

impl std::fmt::Display for DirectoryPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Full => "full",
        })
    }
}

impl std::str::FromStr for DirectoryPermission {
    type Err = crate::UserError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "full" => Ok(Self::Full),
            _ => Err(crate::UserError::InvalidPermission),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_capabilities_do_not_depend_on_enum_order() {
        assert!(DirectoryPermission::Full.allows(DirectoryPermission::Write));
        assert!(DirectoryPermission::Write.allows(DirectoryPermission::Read));
        assert!(!DirectoryPermission::Write.allows(DirectoryPermission::Full));
        assert!(!DirectoryPermission::Read.allows(DirectoryPermission::Write));
        assert_eq!(
            DirectoryPermission::Read.stronger(DirectoryPermission::Full),
            DirectoryPermission::Full
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryGrant {
    user_id: UserId,
    directory: ResourceDirectory,
    permission: DirectoryPermission,
}

impl DirectoryGrant {
    pub fn new(
        user_id: UserId,
        directory: ResourceDirectory,
        permission: DirectoryPermission,
    ) -> Self {
        Self {
            user_id,
            directory,
            permission,
        }
    }
    pub fn user_id(&self) -> UserId {
        self.user_id
    }
    pub fn directory(&self) -> &ResourceDirectory {
        &self.directory
    }
    pub fn permission(&self) -> DirectoryPermission {
        self.permission
    }
}
