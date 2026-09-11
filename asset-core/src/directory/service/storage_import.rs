use super::{DirectoryKernel, DirectoryService};
use crate::{
    CoreError,
    directory::{
        domain::{Directory, DirectoryPath},
        query::LocatedDirectory,
    },
};
use std::sync::Arc;

/// Imports physical directories observed by the storage scanner into Directory aggregates.
#[derive(Clone)]
pub struct DirectoryImportService {
    service: DirectoryService,
}

impl DirectoryImportService {
    pub(super) fn new(kernel: Arc<DirectoryKernel>) -> Self {
        Self {
            service: DirectoryService { kernel },
        }
    }

    /// Import a path that a storage scan has already observed physically.
    pub async fn import_path(&self, path: &DirectoryPath) -> Result<LocatedDirectory, CoreError> {
        self.materialize_path(path).await
    }

    async fn materialize_path(&self, path: &DirectoryPath) -> Result<LocatedDirectory, CoreError> {
        let _guard = self.service.kernel.mutation_lock.lock().await;
        if let Some(directory) = self.service.kernel.read_model.find_by_path(path).await? {
            if !self.service.kernel.storage.directory_exists(path).await? {
                return Err(CoreError::invariant(format!(
                    "storage import path `{path}` no longer exists"
                )));
            }
            return Ok(directory);
        }

        let mut parent = self
            .service
            .kernel
            .read_model
            .find_by_path(&DirectoryPath::root())
            .await?
            .ok_or_else(|| CoreError::invariant("root directory is missing"))?;
        let mut current_path = DirectoryPath::root();
        for name in path.path().split('/').filter(|name| !name.is_empty()) {
            current_path = current_path.child(name)?;
            if let Some(existing) = self
                .service
                .kernel
                .read_model
                .find_by_path(&current_path)
                .await?
            {
                parent = existing;
                continue;
            }

            let physically_existed = self
                .service
                .kernel
                .storage
                .directory_exists(&current_path)
                .await?;
            if !physically_existed {
                return Err(CoreError::invariant(format!(
                    "storage import path `{current_path}` no longer exists"
                )));
            }

            let directory = Directory::new(parent.id(), name)?;
            self.service.kernel.store.insert(&directory).await?;
            self.service.refresh_index(&directory.id()).await?;
            parent = LocatedDirectory::new(directory, current_path.clone());
        }
        Ok(parent)
    }
}
