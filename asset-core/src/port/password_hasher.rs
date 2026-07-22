//! 密码哈希能力端口。

use crate::CoreError;

/// 隔离用户服务与具体密码哈希算法或密码库。
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: &str) -> Result<String, CoreError>;
    fn verify(&self, password: &str, hash: &str) -> Result<bool, CoreError>;
}
