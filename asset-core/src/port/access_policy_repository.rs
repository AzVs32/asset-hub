use crate::{
    CoreError,
    domain::{DirectoryGrant, DirectoryPermission, UserId},
};

#[async_trait::async_trait]
pub trait AccessPolicyRepository: Send + Sync {
    async fn save_grant(&self, grant: &DirectoryGrant) -> Result<(), CoreError>;
    async fn remove_grant(&self, user_id: &UserId, directory: &str) -> Result<(), CoreError>;
    async fn list_grants(&self, user_id: &UserId) -> Result<Vec<DirectoryGrant>, CoreError>;
    async fn effective_permission(
        &self,
        user_id: &UserId,
        directory: &str,
    ) -> Result<Option<DirectoryPermission>, CoreError>;
}
