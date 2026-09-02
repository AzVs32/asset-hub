use crate::{
    CoreError,
    domain::DirectoryId,
    port::{DirectoryIndex, DirectoryStore},
};
use std::sync::Arc;

/// Owns the rebuildable Directory query projection. It is never an authoritative write store.
#[derive(Clone)]
pub struct DirectoryIndexService {
    store: Arc<dyn DirectoryStore>,
    index: Arc<dyn DirectoryIndex>,
}

impl DirectoryIndexService {
    pub(super) fn new(store: Arc<dyn DirectoryStore>, index: Arc<dyn DirectoryIndex>) -> Self {
        Self { store, index }
    }

    pub async fn rebuild(&self) -> Result<(), CoreError> {
        self.index.replace_all(self.store.load_all().await?).await
    }

    pub async fn refresh(&self, id: &DirectoryId) -> Result<(), CoreError> {
        let directory = self
            .store
            .load(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))?;
        if self.index.upsert(directory).await.is_err() {
            return self.rebuild().await;
        }
        Ok(())
    }

    pub(super) async fn remove(&self, id: &DirectoryId) -> Result<(), CoreError> {
        if self.index.remove(id).await.is_err() {
            return self.rebuild().await;
        }
        Ok(())
    }
}
