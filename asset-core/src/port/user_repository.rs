use crate::{
    CoreError,
    domain::{User, UserId},
};

#[async_trait::async_trait]
pub trait UserRepository: Send + Sync {
    async fn save(&self, user: &User) -> Result<(), CoreError>;
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError>;
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError>;
    async fn list(&self) -> Result<Vec<User>, CoreError>;
    async fn count(&self) -> Result<u64, CoreError>;
}
