use super::{ResourceDirectory, UserId};
use serde::{Deserialize, Serialize};

/// 已通过外部入口认证的访问主体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessContext {
    user_id: UserId,
    administrator: bool,
    home_directory: ResourceDirectory,
}

impl AccessContext {
    pub fn member(user_id: UserId, home_directory: ResourceDirectory) -> Self {
        Self {
            user_id,
            administrator: false,
            home_directory,
        }
    }
    pub fn administrator(user_id: UserId) -> Self {
        Self {
            user_id,
            administrator: true,
            home_directory: ResourceDirectory::root(),
        }
    }
    pub fn user_id(&self) -> UserId {
        self.user_id
    }
    pub fn is_administrator(&self) -> bool {
        self.administrator
    }
    pub fn home_directory(&self) -> &ResourceDirectory {
        &self.home_directory
    }
}

/// 目录权限按 Manage > Write > Read 排序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryPermission {
    Read,
    Write,
    Manage,
}

impl std::fmt::Display for DirectoryPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Manage => "manage",
        })
    }
}

impl std::str::FromStr for DirectoryPermission {
    type Err = crate::UserError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "manage" => Ok(Self::Manage),
            _ => Err(crate::UserError::InvalidPermission),
        }
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
