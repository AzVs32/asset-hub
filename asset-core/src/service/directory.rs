//! Directory aggregate application service.

mod command;
mod contract;
mod index;
mod provisioning;
mod secured;

pub use contract::UpdateDirectory;
pub use index::DirectoryIndexService;
pub use provisioning::DirectoryProvisioningService;
pub use secured::SecuredDirectoryService;

use crate::{
    CoreError,
    domain::{DirectoryId, DirectoryPath},
    port::{
        DirectoryIndex, DirectoryLocation, DirectoryProjection, DirectoryQuery,
        DirectoryRelocationStore, DirectoryStorage, DirectoryStore, LocatedDirectory,
    },
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Coordinates directory aggregates, the durable store, the query index, and physical storage.
#[derive(Clone)]
pub struct DirectoryService {
    kernel: Arc<DirectoryKernel>,
}

struct DirectoryKernel {
    store: Arc<dyn DirectoryStore>,
    query: Arc<dyn DirectoryQuery>,
    storage: Arc<dyn DirectoryStorage>,
    relocations: Arc<dyn DirectoryRelocationStore>,
    mutation_lock: Arc<Mutex<()>>,
    index_service: DirectoryIndexService,
}

/// Composition-time bundle that guarantees all Directory services share one mutation boundary.
pub struct DirectoryServices {
    directory: DirectoryService,
    provisioning: DirectoryProvisioningService,
    index: DirectoryIndexService,
}

impl DirectoryServices {
    pub fn new(
        store: Arc<dyn DirectoryStore>,
        index: Arc<dyn DirectoryProjection>,
        storage: Arc<dyn DirectoryStorage>,
        relocations: Arc<dyn DirectoryRelocationStore>,
    ) -> Self {
        let query: Arc<dyn DirectoryQuery> = index.clone();
        let index_writer: Arc<dyn DirectoryIndex> = index;
        let index_service = DirectoryIndexService::new(store.clone(), index_writer);
        let kernel = Arc::new(DirectoryKernel {
            store,
            query,
            storage,
            relocations,
            mutation_lock: Arc::new(Mutex::new(())),
            index_service: index_service.clone(),
        });
        Self {
            directory: DirectoryService {
                kernel: kernel.clone(),
            },
            provisioning: DirectoryProvisioningService::new(kernel),
            index: index_service,
        }
    }

    pub fn directory_service(&self) -> DirectoryService {
        self.directory.clone()
    }

    pub fn provisioning_service(&self) -> DirectoryProvisioningService {
        self.provisioning.clone()
    }

    pub fn index_service(&self) -> DirectoryIndexService {
        self.index.clone()
    }
}

impl DirectoryService {
    pub fn secured<'a>(
        &'a self,
        authorization: &'a crate::service::AuthorizationService,
        context: &'a crate::domain::AccessContext,
    ) -> SecuredDirectoryService<'a> {
        SecuredDirectoryService::new(self, authorization, context)
    }

    pub async fn root(&self) -> Result<DirectoryLocation, CoreError> {
        Ok(self
            .find_by_id(&DirectoryId::root())
            .await?
            .location()
            .clone())
    }

    pub async fn find_by_id(&self, id: &DirectoryId) -> Result<LocatedDirectory, CoreError> {
        self.kernel
            .query
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))
    }

    pub async fn locate_by_id(&self, id: &DirectoryId) -> Result<DirectoryLocation, CoreError> {
        Ok(self.find_by_id(id).await?.location().clone())
    }

    pub async fn find_by_path(&self, path: &DirectoryPath) -> Result<LocatedDirectory, CoreError> {
        self.kernel
            .query
            .find_by_path(path)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", path.path()))
    }

    pub async fn resolve_path(&self, path: &DirectoryPath) -> Result<DirectoryLocation, CoreError> {
        Ok(self.find_by_path(path).await?.location().clone())
    }

    pub async fn list_children(
        &self,
        parent_id: &DirectoryId,
    ) -> Result<Vec<DirectoryLocation>, CoreError> {
        Ok(self
            .list_located_children(parent_id)
            .await?
            .into_iter()
            .map(|directory| directory.location().clone())
            .collect())
    }

    pub async fn list_located_children(
        &self,
        parent_id: &DirectoryId,
    ) -> Result<Vec<LocatedDirectory>, CoreError> {
        self.kernel.query.list_children(parent_id).await
    }

    pub async fn contains(
        &self,
        ancestor: &DirectoryId,
        candidate: &DirectoryId,
    ) -> Result<bool, CoreError> {
        self.kernel
            .query
            .is_descendant_or_self(ancestor, candidate)
            .await
    }

    async fn refresh_index(&self, id: &DirectoryId) -> Result<(), CoreError> {
        self.kernel.index_service.refresh(id).await
    }
}
