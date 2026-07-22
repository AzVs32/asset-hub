//! 用户聚合持久化与密码能力端口。

use crate::{
    CoreError,
    domain::{User, UserId},
};

/// 保存和还原完整用户聚合，不承担密码哈希职责。
#[async_trait::async_trait]
pub trait UserRepository: Send + Sync {
    /// 原子创建用户及其工作目录记录。
    async fn create(&self, user: &User) -> Result<(), CoreError>;
    /// 保存既有用户的可变状态。
    async fn save(&self, user: &User) -> Result<(), CoreError>;
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError>;
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError>;
    async fn list(&self) -> Result<Vec<User>, CoreError>;
    async fn count(&self) -> Result<u64, CoreError>;
}

/// 隔离用户服务与具体密码哈希算法或密码库。
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: &str) -> Result<String, CoreError>;
    fn verify(&self, password: &str, hash: &str) -> Result<bool, CoreError>;
}
