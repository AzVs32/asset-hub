//! 用户聚合持久化与密码能力端口。

use crate::{
    CoreError,
    domain::{User, UserId},
    port::DirectoryLocation,
};

/// 带当前工作目录位置的用户读取投影。
#[derive(Debug, Clone)]
pub struct LocatedUser {
    user: User,
    workspace: DirectoryLocation,
}

impl LocatedUser {
    pub fn new(user: User, workspace: DirectoryLocation) -> Result<Self, CoreError> {
        if user.workspace_directory_id() != workspace.id() {
            return Err(CoreError::configuration(
                "user workspace does not match its location projection",
            ));
        }
        Ok(Self { user, workspace })
    }

    pub fn user(&self) -> &User {
        &self.user
    }

    pub fn workspace(&self) -> &DirectoryLocation {
        &self.workspace
    }

    pub fn into_user(self) -> User {
        self.user
    }

    pub fn into_parts(self) -> (User, DirectoryLocation) {
        (self.user, self.workspace)
    }
}

/// 用户读取查询，不参与身份验证和聚合保存。
#[async_trait::async_trait]
pub trait UserQuery: Send + Sync {
    async fn find_located_by_id(&self, id: &UserId) -> Result<Option<LocatedUser>, CoreError>;
    async fn find_located_by_username(
        &self,
        username: &str,
    ) -> Result<Option<LocatedUser>, CoreError>;
    async fn list_located(&self) -> Result<Vec<LocatedUser>, CoreError>;
}

/// 保存和还原完整用户聚合，不承担密码哈希职责。
#[async_trait::async_trait]
pub trait UserRepository: Send + Sync {
    /// 创建引用既有工作目录的用户；工作目录由应用服务预先确保。
    async fn create(&self, user: &User) -> Result<(), CoreError>;
    /// 保存既有用户的可变状态。
    async fn save(&self, user: &User) -> Result<(), CoreError>;
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError>;
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError>;
    async fn count(&self) -> Result<u64, CoreError>;
}

/// 隔离用户服务与具体密码哈希算法或密码库。
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: &str) -> Result<String, CoreError>;
    fn verify(&self, password: &str, hash: &str) -> Result<bool, CoreError>;
}
