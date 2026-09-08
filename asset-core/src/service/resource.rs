//! Resource, content, upload, and storage-maintenance application services.
//!
//! These services share narrow ports and path locks but do not borrow a single facade containing
//! every dependency. `ResourceService` itself owns only Resource metadata/lifecycle coordination.

use crate::CoreError;
use crate::domain::ResourceContentEditPolicy;
use crate::port::{
    BlobHealth, ContentObjectStore, ContentReader, ContentStagingStore, IdempotencyRepository,
    ResourceContentReplacementRepository, ResourceDeletionRepository, ResourceMaintenanceReadModel,
    ResourceReadModel, ResourceRelocationStore, ResourceStore, StorageScanner,
    UploadSessionRepository,
};
use crate::service::{
    DirectoryImportService, DirectoryIndexService, DirectoryService, IdempotencyService,
};
use std::sync::Arc;
use std::time::Duration;

mod command;
mod content;
mod contract;
mod path_resolver;
mod reconciliation;
mod storage_key_locks;
mod upload;
mod upload_locks;

pub use content::ContentService;
pub use contract::{CreateUpload, ReplaceResourceContent, ResourceContentStream, UpdateResource};
pub use reconciliation::{
    ResourceScanProgress, StorageMaintenanceService, StorageReconciliationReport,
};
pub use upload::UploadService;

pub(crate) use command::build_resource;
pub(crate) use storage_key_locks::StorageKeyLocks;
pub(crate) use upload_locks::UploadLocks;

/// Resource metadata and lifecycle queries, listing, updates, and deletion.
#[derive(Clone)]
pub struct ResourceService {
    pub(crate) store: Arc<dyn ResourceStore>,
    pub(crate) read_model: Arc<dyn ResourceReadModel>,
    pub(crate) objects: Arc<dyn ContentObjectStore>,
    pub(crate) relocations: Arc<dyn ResourceRelocationStore>,
    pub(crate) deletions: Arc<dyn ResourceDeletionRepository>,
    pub(crate) directories: DirectoryService,
    pub(crate) storage_key_locks: Arc<StorageKeyLocks>,
}

impl ResourceService {
    pub(crate) fn new(
        store: Arc<dyn ResourceStore>,
        read_model: Arc<dyn ResourceReadModel>,
        objects: Arc<dyn ContentObjectStore>,
        relocations: Arc<dyn ResourceRelocationStore>,
        deletions: Arc<dyn ResourceDeletionRepository>,
        directories: DirectoryService,
        storage_key_locks: Arc<StorageKeyLocks>,
    ) -> Self {
        Self {
            store,
            read_model,
            objects,
            relocations,
            deletions,
            directories,
            storage_key_locks,
        }
    }
}

/// Deterministic assembly bundle for the five independent Resource-related services. It is the
/// only public constructor that creates their shared ordered path-lock registry.
pub struct ResourceServices {
    resources: ResourceService,
    content: ContentService,
    uploads: UploadService,
    maintenance: StorageMaintenanceService,
    idempotency: IdempotencyService,
}

impl ResourceServices {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<dyn ResourceStore>,
        read_model: Arc<dyn ResourceReadModel>,
        maintenance_read_model: Arc<dyn ResourceMaintenanceReadModel>,
        relocation_store: Arc<dyn ResourceRelocationStore>,
        deletion_repository: Arc<dyn ResourceDeletionRepository>,
        content_reader: Arc<dyn ContentReader>,
        content_staging: Arc<dyn ContentStagingStore>,
        content_objects: Arc<dyn ContentObjectStore>,
        blob_health: Arc<dyn BlobHealth>,
        storage_scanner: Arc<dyn StorageScanner>,
        directories: DirectoryService,
        directory_index: DirectoryIndexService,
        directory_import: DirectoryImportService,
        upload_sessions: Arc<dyn UploadSessionRepository>,
        content_replacements: Arc<dyn ResourceContentReplacementRepository>,
        edit_policy: Arc<ResourceContentEditPolicy>,
        idempotency_repository: Arc<dyn IdempotencyRepository>,
        idempotency_lease_duration: Duration,
    ) -> Result<Self, CoreError> {
        let locks = Arc::new(StorageKeyLocks::default());
        let idempotency = IdempotencyService::with_lease_duration(
            idempotency_repository,
            idempotency_lease_duration,
        )?;
        let resources = ResourceService::new(
            store.clone(),
            read_model.clone(),
            content_objects.clone(),
            relocation_store,
            deletion_repository,
            directories.clone(),
            locks.clone(),
        );
        let content = ContentService::new(
            read_model.clone(),
            store.clone(),
            content_reader.clone(),
            content_staging.clone(),
            content_objects.clone(),
            content_replacements,
            locks.clone(),
            edit_policy.clone(),
            idempotency.clone(),
        );
        let uploads = UploadService::new(
            store.clone(),
            read_model.clone(),
            content_staging.clone(),
            content_reader.clone(),
            content_objects.clone(),
            storage_scanner.clone(),
            directories.clone(),
            upload_sessions,
            locks.clone(),
            idempotency.clone(),
        );
        let maintenance = StorageMaintenanceService::new(
            store,
            read_model,
            maintenance_read_model,
            storage_scanner,
            content_reader,
            blob_health,
            directories,
            directory_index,
            directory_import,
            locks,
        );
        Ok(Self {
            resources,
            content,
            uploads,
            maintenance,
            idempotency,
        })
    }

    pub fn resource_service(&self) -> ResourceService {
        self.resources.clone()
    }
    pub fn content_service(&self) -> ContentService {
        self.content.clone()
    }
    pub fn upload_service(&self) -> UploadService {
        self.uploads.clone()
    }
    pub fn storage_maintenance_service(&self) -> StorageMaintenanceService {
        self.maintenance.clone()
    }
    pub fn idempotency_service(&self) -> IdempotencyService {
        self.idempotency.clone()
    }
}

// Focused recovery and adapter tests live beside the owning relocation/store implementations.
