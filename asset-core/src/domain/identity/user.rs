use crate::domain::DirectoryId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

crate::gen_id_uuid_v7!(UserId);

const MAX_USERNAME_LEN: usize = 64;

/// 可用于登录和授权的用户聚合根。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    Active,
    Disabled,
}

#[derive(Debug, Clone)]
pub struct UserSnapshot {
    pub id: UserId,
    pub username: String,
    pub credential_hash: String,
    pub role: UserRole,
    pub status: UserStatus,
    pub workspace_directory_id: DirectoryId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    pub fn new(
        username: impl Into<String>,
        credential_hash: impl Into<String>,
        role: UserRole,
        workspace_directory_id: DirectoryId,
    ) -> Result<Self, crate::UserError> {
        let now = Utc::now();
        Self::rehydrate(UserSnapshot {
            id: UserId::new(),
            username: username.into(),
            credential_hash: credential_hash.into(),
            role,
            status: UserStatus::Active,
            workspace_directory_id,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn rehydrate(snapshot: UserSnapshot) -> Result<Self, crate::UserError> {
        let username = normalize_username(snapshot.username)?;
        if snapshot.credential_hash.trim().is_empty() {
            return Err(crate::UserError::InvalidCredentialHash);
        }
        Ok(Self {
            id: snapshot.id,
            username,
            credential_hash: snapshot.credential_hash,
            role: snapshot.role,
            status: snapshot.status,
            workspace_directory_id: snapshot.workspace_directory_id,
            created_at: snapshot.created_at,
            updated_at: snapshot.updated_at,
        })
    }

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
