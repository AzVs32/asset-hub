//! Directory aggregate application service.

mod command;
mod contract;
mod index;
mod maintenance;
mod recovery;
mod storage_import;

pub use contract::UpdateDirectory;
pub use index::DirectoryIndexService;
pub use maintenance::DirectoryMaintenanceService;
pub use recovery::DirectoryRecoveryService;
pub use storage_import::DirectoryImportService;

use crate::{
    CoreError,
    directory::{
        domain::{DirectoryId, DirectoryPath},
        port::{
            DirectoryIndex, DirectoryProjection, DirectoryQuery, DirectoryRelocationStore,
            DirectoryStore,
        },
        query::{DirectoryLocation, LocatedDirectory},
    },
    storage::port::DirectoryStorage,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Coordinates global directory aggregates, the durable store, the query index, and physical
/// storage.
///
/// Its public query and mutation use cases use stable IDs or global [`DirectoryPath`] values.
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
    maintenance: DirectoryMaintenanceService,
    recovery: DirectoryRecoveryService,
    storage_import: DirectoryImportService,
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
        let directory = DirectoryService {
            kernel: kernel.clone(),
        };
        Self {
            maintenance: DirectoryMaintenanceService::new(directory.clone()),
            recovery: DirectoryRecoveryService::new(directory.clone()),
            directory,
            storage_import: DirectoryImportService::new(kernel),
            index: index_service,
        }
    }

    pub fn directory_service(&self) -> DirectoryService {
        self.directory.clone()
    }

    /// Return trusted storage-reconciliation operations that share the Directory mutation lock.
    pub fn maintenance_service(&self) -> DirectoryMaintenanceService {
        self.maintenance.clone()
    }

    /// Return the explicit process-lifecycle recovery interface.
    pub fn recovery_service(&self) -> DirectoryRecoveryService {
        self.recovery.clone()
    }

    pub fn storage_import_service(&self) -> DirectoryImportService {
        self.storage_import.clone()
    }

    pub fn index_service(&self) -> DirectoryIndexService {
        self.index.clone()
    }
}

impl DirectoryService {
    /// Locate the global root, whose identity is the nil UUID and whose path is empty.
    pub async fn root(&self) -> Result<DirectoryLocation, CoreError> {
        Ok(self
            .find_by_id(&DirectoryId::root())
            .await?
            .location()
            .clone())
    }

    /// Find a directory by its global stable ID.
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

    /// Find a directory by its global canonical path.
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
    ) -> Result<Vec<LocatedDirectory>, CoreError> {
        self.find_by_id(parent_id).await?;
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
