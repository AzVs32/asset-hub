use crate::domain::DirectoryId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Formatter;

crate::gen_id_uuid_v7!(UserId);

const MAX_USERNAME_LEN: usize = 64;

/// 可用于登录和授权的用户聚合根。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct User {
    id: UserId,
    username: String,
    credential_hash: String,
    role: UserRole,
    status: UserStatus,
    /// 登录后的默认入口目录；可被多个用户共享，本身不产生任何权限。
    workspace_directory_id: DirectoryId,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Administrator,
    Member,
}

impl fmt::Display for UserRole {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Administrator => "administrator",
            Self::Member => "member",
        };
        write!(f, "{}", name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    Active,
    Disabled,
}

impl fmt::Display for UserStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        };
        write!(f, "{}", name)
    }
}

impl User {
    pub fn new(
        username: impl Into<String>,
        credential_hash: impl Into<String>,
        role: UserRole,
        workspace_directory_id: DirectoryId,
    ) -> Result<Self, crate::UserError> {
        let now = Utc::now();
        Self::rehydrate(
            UserId::new(),
            username.into(),
            credential_hash.into(),
            role,
            UserStatus::Active,
            workspace_directory_id,
            now,
            now,
        )
    }

    /// 从持久化适配器已解析的完整状态还原用户聚合。
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: UserId,
        username: String,
        credential_hash: String,
        role: UserRole,
        status: UserStatus,
        workspace_directory_id: DirectoryId,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, crate::UserError> {
        let username = normalize_username(username)?;
        if credential_hash.trim().is_empty() {
            return Err(crate::UserError::InvalidCredentialHash);
        }
        if updated_at < created_at {
            return Err(crate::UserError::InvalidTimestamps);
        }
        Ok(Self {
            id,
            username,
            credential_hash,
            role,
            status,
            workspace_directory_id,
            created_at,
            updated_at,
        })
    }
}

impl User {
    pub fn id(&self) -> UserId {
        self.id
    }
    pub fn username(&self) -> &str {
        &self.username
    }
    pub fn credential_hash(&self) -> &str {
        &self.credential_hash
    }
    pub fn role(&self) -> UserRole {
        self.role
    }
    pub fn status(&self) -> UserStatus {
        self.status
    }
    pub fn workspace_directory_id(&self) -> DirectoryId {
        self.workspace_directory_id
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    pub fn is_administrator(&self) -> bool {
        self.role == UserRole::Administrator
    }
    pub fn is_active(&self) -> bool {
        self.status == UserStatus::Active
    }
    pub fn change_status(&mut self, status: UserStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }
    pub fn change_credential_hash(
        &mut self,
        credential_hash: impl Into<String>,
    ) -> Result<(), crate::UserError> {
        let credential_hash = credential_hash.into();
        if credential_hash.trim().is_empty() {
            return Err(crate::UserError::InvalidCredentialHash);
        }
        self.credential_hash = credential_hash;
        self.updated_at = Utc::now();
        Ok(())
    }
}

fn normalize_username(value: String) -> Result<String, crate::UserError> {
    let value = value.trim();
    if value.len() < 3 || value.len() > MAX_USERNAME_LEN {
        return Err(crate::UserError::InvalidUsername);
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(crate::UserError::InvalidUsername);
    }
    Ok(value.to_owned())
}
