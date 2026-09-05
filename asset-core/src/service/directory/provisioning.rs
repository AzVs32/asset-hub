use super::{DirectoryKernel, DirectoryService};
use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryPath},
    port::{DirectoryLocation, LocatedDirectory},
};
use std::sync::Arc;

/// Trusted system provisioning and storage-import use cases.
///
/// User-facing Resource, Upload, and Directory commands must not depend on this service.
#[derive(Clone)]
pub struct DirectoryProvisioningService {
    service: DirectoryService,
}

impl DirectoryProvisioningService {
    pub(super) fn new(kernel: Arc<DirectoryKernel>) -> Self {
        Self {
            service: DirectoryService { kernel },
        }
    }

    /// Create missing aggregate and physical path segments for trusted system provisioning.
    pub async fn provision_path(
        &self,
        path: &DirectoryPath,
    ) -> Result<DirectoryLocation, CoreError> {
        self.materialize_path(path, false).await
    }

    /// Import a path that a storage scan has already observed physically.
    pub async fn import_storage_path(
        &self,
        path: &DirectoryPath,
    ) -> Result<DirectoryLocation, CoreError> {
        self.materialize_path(path, true).await
    }

    async fn materialize_path(
        &self,
        path: &DirectoryPath,
        importing: bool,
    ) -> Result<DirectoryLocation, CoreError> {
        let _guard = self.service.kernel.mutation_lock.lock().await;
        if let Some(directory) = self.service.kernel.query.find_by_path(path).await? {
            if importing {
                if !self.service.kernel.storage.directory_exists(path).await? {
                    return Err(CoreError::invariant(format!(
                        "storage import path `{path}` no longer exists"
                    )));
                }
            } else {
                self.service.kernel.storage.ensure_directory(path).await?;
            }
            return Ok(directory.location().clone());
        }

        let mut parent = self
            .service
            .kernel
            .query
            .find_by_id(&DirectoryId::root())
            .await?
            .ok_or_else(|| CoreError::invariant("root directory is missing"))?;
        let mut current_path = DirectoryPath::root();
        for name in path.path().split('/').filter(|name| !name.is_empty()) {
            current_path = current_path.child(name)?;
            if let Some(existing) = self
                .service
                .kernel
                .query
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
            if importing && !physically_existed {
                return Err(CoreError::invariant(format!(
                    "storage import path `{current_path}` no longer exists"
                )));
            }
            if !importing {
                self.service
                    .kernel
                    .storage
                    .ensure_directory(&current_path)
                    .await?;
            }

            let directory = Directory::new(parent.id(), name)?;
            if let Err(error) = self.service.kernel.store.insert(&directory).await {
                if !physically_existed {
                    let _ = self
                        .service
                        .kernel
                        .storage
                        .delete_empty_directory(&current_path)
                        .await;
                }
                return Err(error);
            }
            self.service.refresh_index(&directory.id()).await?;
            parent = LocatedDirectory::new(
                directory.clone(),
                DirectoryLocation::new(directory.id(), current_path.clone()),
            )?;
        }
        Ok(parent.location().clone())
    }
}
