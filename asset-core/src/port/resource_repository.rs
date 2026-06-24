use crate::CoreError;
use crate::domain::{Resource, ResourceId};

#[async_trait::async_trait]
pub trait ResourceRepository: Send + Sync {
    async fn save(&self, resource: &Resource) -> Result<(), CoreError>;

    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError>;

    async fn delete(&self, id: &ResourceId) -> Result<(), CoreError>;
}
