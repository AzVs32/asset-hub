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
            return Err(CoreError::invariant(
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

/// 用户读取投影端口，不参与身份验证和聚合保存。
///
/// 查询适配器负责把用户聚合与其当前工作区目录位置组合为一致的 `LocatedUser`。
#[async_trait::async_trait]
pub trait UserQuery: Send + Sync {
    /// 按用户 ID 查询用户及其工作区位置；不存在时返回 `None`。
    async fn find_located_by_id(&self, id: &UserId) -> Result<Option<LocatedUser>, CoreError>;

    /// 按规范用户名查询用户及其工作区位置；不存在时返回 `None`。
    async fn find_located_by_username(
        &self,
        username: &str,
    ) -> Result<Option<LocatedUser>, CoreError>;

    /// 返回全部用户及其工作区位置。
    async fn list_located(&self) -> Result<Vec<LocatedUser>, CoreError>;
}

/// 保存和还原完整用户聚合的持久化端口，不承担密码哈希职责。
#[async_trait::async_trait]
pub trait UserRepository: Send + Sync {
    /// 创建引用既有工作目录的用户；工作目录由应用服务预先确保。
    async fn create(&self, user: &User) -> Result<(), CoreError>;
    /// 保存既有用户的可变状态。
    async fn save(&self, user: &User) -> Result<(), CoreError>;

    /// 按用户 ID 还原完整聚合；不存在时返回 `None`。
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError>;

    /// 按规范用户名还原完整聚合；不存在时返回 `None`。
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError>;
}

/// 隔离用户服务与具体密码哈希算法或密码库的基础设施端口。
pub trait PasswordHasher: Send + Sync {
    /// 生成可安全持久化的密码哈希；不得返回或嵌入明文密码。
    fn hash(&self, password: &str) -> Result<String, CoreError>;

    /// 校验明文密码；不匹配返回 `Ok(false)`，哈希格式或算法错误返回 `Err`。
    fn verify(&self, password: &str, hash: &str) -> Result<bool, CoreError>;
}
