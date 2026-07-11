use super::UserId;
use serde::{Deserialize, Serialize};

/// 已通过外部入口认证的访问主体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessContext {
    user_id: UserId,
    administrator: bool,
}

impl AccessContext {
    pub fn user(user_id: UserId) -> Self {
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
    directory: String,
    permission: DirectoryPermission,
}

impl DirectoryGrant {
    pub fn new(
        user_id: UserId,
        directory: impl Into<String>,
        permission: DirectoryPermission,
    ) -> Result<Self, crate::UserError> {
        Ok(Self {
            user_id,
            directory: normalize_directory(directory.into())?,
            permission,
        })
    }
    pub fn user_id(&self) -> UserId {
        self.user_id
    }
    pub fn directory(&self) -> &str {
        &self.directory
    }
    pub fn permission(&self) -> DirectoryPermission {
        self.permission
    }
}

pub fn normalize_directory(value: impl Into<String>) -> Result<String, crate::UserError> {
    let value = value.into();
    let value = value.trim().trim_matches('/');
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.len() > 1024
        || value
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(crate::UserError::InvalidDirectory);
    }
    Ok(value.to_owned())
}
