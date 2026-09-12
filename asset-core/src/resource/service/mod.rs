//! Resource, content, upload, and storage-maintenance application services.
//!
//! These services share narrow ports and path locks but do not borrow a single facade containing
//! every dependency. `ResourceService` itself owns only Resource metadata/lifecycle coordination.

use crate::{
    directory::service::{
        DirectoryImportService, DirectoryIndexService, DirectoryMaintenanceService,
        DirectoryService,
    },
    idempotency::service::IdempotencyService,
    resource::port::{
        ResourceContentReplacementStore, ResourceDeletionStore, ResourceMaintenanceReadModel,
        ResourceReadModel, ResourceRelocationStore, ResourceStore, UploadSessionStore,
    },
    storage::port::{
        BlobHealth, ContentObjectStore, ContentReader, ContentStagingStore, StorageScanner,
    },
};
use std::sync::Arc;

mod command;
mod content;
mod contract;
mod path_resolver;
mod reconciliation;
mod storage_key_locks;
mod upload;
mod upload_locks;

pub use command::ResourceRecoveryService;
pub use content::{ContentRecoveryService, ContentService};
pub use contract::{
    CreateContentReplacementUpload, CreateUpload, ResourceContentStream, UpdateResource,
};
pub use reconciliation::{
    ResourceScanProgress, StorageHealthService, StorageMaintenanceService,
    StorageReconciliationReport,
};
pub use upload::{UploadFinalizationService, UploadService};

use command::build_resource;
use storage_key_locks::StorageKeyLocks;
use upload_locks::UploadLocks;

/// Resource metadata and lifecycle queries, listing, updates, and deletion.
#[derive(Clone)]
pub struct ResourceService {
    store: Arc<dyn ResourceStore>,
    read_model: Arc<dyn ResourceReadModel>,
    objects: Arc<dyn ContentObjectStore>,
    relocations: Arc<dyn ResourceRelocationStore>,
    deletions: Arc<dyn ResourceDeletionStore>,
    directories: DirectoryService,
    storage_key_locks: Arc<StorageKeyLocks>,
}

impl ResourceService {
    fn new(
        store: Arc<dyn ResourceStore>,
        read_model: Arc<dyn ResourceReadModel>,
        objects: Arc<dyn ContentObjectStore>,
        relocations: Arc<dyn ResourceRelocationStore>,
        deletions: Arc<dyn ResourceDeletionStore>,
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
    resource_recovery: ResourceRecoveryService,
    content_recovery: ContentRecoveryService,
    upload_finalization: UploadFinalizationService,
    storage_health: StorageHealthService,
}

impl ResourceServices {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<dyn ResourceStore>,
        read_model: Arc<dyn ResourceReadModel>,
        maintenance_read_model: Arc<dyn ResourceMaintenanceReadModel>,
        relocation_store: Arc<dyn ResourceRelocationStore>,
        deletion_store: Arc<dyn ResourceDeletionStore>,
        content_reader: Arc<dyn ContentReader>,
        content_staging: Arc<dyn ContentStagingStore>,
        content_objects: Arc<dyn ContentObjectStore>,
        blob_health: Arc<dyn BlobHealth>,
        storage_scanner: Arc<dyn StorageScanner>,
        directories: DirectoryService,
        directory_maintenance: DirectoryMaintenanceService,
        directory_index: DirectoryIndexService,
        directory_import: DirectoryImportService,
        upload_sessions: Arc<dyn UploadSessionStore>,
        content_replacements: Arc<dyn ResourceContentReplacementStore>,
        idempotency: IdempotencyService,
    ) -> Self {
        let locks = Arc::new(StorageKeyLocks::default());
        let resources = ResourceService::new(
            store.clone(),
            read_model.clone(),
            content_objects.clone(),
            relocation_store,
            deletion_store,
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
            content.clone(),
            idempotency.clone(),
        );
        let maintenance = StorageMaintenanceService::new(
            store,
            read_model,
            maintenance_read_model,
            storage_scanner,
            content_reader,
            directories,
            directory_maintenance,
            directory_index,
            directory_import,
            locks,
        );
        let resource_recovery = ResourceRecoveryService::new(resources.clone());
        let content_recovery = ContentRecoveryService::new(content.clone());
        let upload_finalization = UploadFinalizationService::new(uploads.clone());
        let storage_health = StorageHealthService::new(blob_health);
        Self {
            resources,
            content,
            uploads,
            maintenance,
            resource_recovery,
            content_recovery,
            upload_finalization,
            storage_health,
        }
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
    /// Return the explicit recovery interface for Resource metadata lifecycle intents.
    pub fn resource_recovery_service(&self) -> ResourceRecoveryService {
        self.resource_recovery.clone()
    }
    /// Return the explicit recovery interface for Resource content replacement intents.
    pub fn content_recovery_service(&self) -> ContentRecoveryService {
        self.content_recovery.clone()
    }
    /// Return the background finalization interface for uploads in `Finalizing` state.
    pub fn upload_finalization_service(&self) -> UploadFinalizationService {
        self.upload_finalization.clone()
    }
    /// Return the narrow Blob readiness interface required by HTTP composition.
    pub fn storage_health_service(&self) -> StorageHealthService {
        self.storage_health.clone()
    }
}

// Focused recovery and adapter tests live beside the owning relocation/store implementations.
