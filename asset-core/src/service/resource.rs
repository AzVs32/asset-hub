//! Resource, content, upload, action, and storage-maintenance application services.
//!
//! These services share narrow ports and path locks but do not borrow a single facade containing
//! every dependency. `ResourceService` itself owns only Resource metadata/lifecycle coordination.

use crate::CoreError;
use crate::domain::{
    ResourceActionPolicy, ResourceContentEditPolicy, ResourceKind, ResourceKindDefinition,
};
use crate::port::{
    BlobHealth, ContentObjectStore, ContentReader, ContentStagingStore, ResourceActionExecutor,
    ResourceActionRegistry, ResourceContentReplacementRepository, ResourceKindRegistry,
    ResourceMaintenanceReadModel, ResourceReadModel, ResourceRelocationStore, ResourceStore,
    StorageScanner, UploadSessionRepository,
};
use crate::service::{DirectoryProvisioningService, DirectoryService};
use std::sync::Arc;

mod action;
mod command;
mod content;
mod contract;
mod path_resolver;
mod reconciliation;
mod secured;
mod storage_key_locks;
mod upload;
mod upload_locks;

pub use action::{ActionOrchestrator, SecuredActionOrchestrator};
pub use content::{ContentService, SecuredContentService};
pub use contract::{
    CreateUpload, ExecuteResourceAction, ReplaceResourceContent, ResourceActions,
    ResourceContentStream, UpdateResource,
};
pub use reconciliation::{
    ResourceScanProgress, StorageMaintenanceService, StorageReconciliationReport,
};
pub use secured::SecuredResourceService;
pub use upload::{SecuredUploadService, UploadService};

pub(crate) use command::build_resource;
pub(crate) use storage_key_locks::StorageKeyLocks;
pub(crate) use upload_locks::UploadLocks;

/// Resource metadata and lifecycle service.
#[derive(Clone)]
pub struct ResourceService {
    pub(crate) store: Arc<dyn ResourceStore>,
    pub(crate) read_model: Arc<dyn ResourceReadModel>,
    pub(crate) objects: Arc<dyn ContentObjectStore>,
    pub(crate) relocations: Arc<dyn ResourceRelocationStore>,
    pub(crate) directories: DirectoryService,
    pub(crate) kind_registry: Arc<dyn ResourceKindRegistry>,
    pub(crate) storage_key_locks: Arc<StorageKeyLocks>,
}

impl ResourceService {
    pub(crate) fn new(
        store: Arc<dyn ResourceStore>,
        read_model: Arc<dyn ResourceReadModel>,
        objects: Arc<dyn ContentObjectStore>,
        relocations: Arc<dyn ResourceRelocationStore>,
        directories: DirectoryService,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        storage_key_locks: Arc<StorageKeyLocks>,
    ) -> Self {
        Self {
            store,
            read_model,
            objects,
            relocations,
            directories,
            kind_registry,
            storage_key_locks,
        }
    }

    pub fn secured<'a>(
        &'a self,
        authorization: &'a crate::service::AuthorizationService,
        context: &'a crate::domain::AccessContext,
    ) -> SecuredResourceService<'a> {
        SecuredResourceService::new(self, authorization, context)
    }

    pub fn kind_definitions(&self) -> &[ResourceKindDefinition] {
        self.kind_registry.definitions()
    }

    pub fn kind_lineage(&self, kind: &ResourceKind) -> Vec<ResourceKind> {
        self.kind_registry.lineage(kind)
    }

    pub(crate) fn validate_registered_kind(
        &self,
        kind: Option<ResourceKind>,
    ) -> Result<ResourceKind, CoreError> {
        let kind = kind.unwrap_or_default();
        if self.kind_registry.supports(&kind) {
            Ok(kind)
        } else {
            Err(CoreError::unsupported("resource kind", kind.to_string()))
        }
    }
}

/// Deterministic assembly bundle for the five independent Resource-related services. It is the
/// only public constructor that creates their shared ordered path-lock registry.
pub struct ResourceServices {
    resources: ResourceService,
    content: ContentService,
    uploads: UploadService,
    actions: ActionOrchestrator,
    maintenance: StorageMaintenanceService,
}

impl ResourceServices {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<dyn ResourceStore>,
        read_model: Arc<dyn ResourceReadModel>,
        maintenance_read_model: Arc<dyn ResourceMaintenanceReadModel>,
        relocation_store: Arc<dyn ResourceRelocationStore>,
        content_reader: Arc<dyn ContentReader>,
        content_staging: Arc<dyn ContentStagingStore>,
        content_objects: Arc<dyn ContentObjectStore>,
        blob_health: Arc<dyn BlobHealth>,
        storage_scanner: Arc<dyn StorageScanner>,
        directories: DirectoryService,
        directory_provisioning: DirectoryProvisioningService,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        upload_sessions: Arc<dyn UploadSessionRepository>,
        content_replacements: Arc<dyn ResourceContentReplacementRepository>,
        action_registry: Arc<dyn ResourceActionRegistry>,
        action_executor: Arc<dyn ResourceActionExecutor>,
        action_policy: Arc<ResourceActionPolicy>,
        edit_policy: Arc<ResourceContentEditPolicy>,
    ) -> Self {
        let locks = Arc::new(StorageKeyLocks::default());
        let resources = ResourceService::new(
            store.clone(),
            read_model.clone(),
            content_objects.clone(),
            relocation_store,
            directories.clone(),
            kind_registry.clone(),
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
        );
        let uploads = UploadService::new(
            store.clone(),
            read_model.clone(),
            content_staging.clone(),
            content_reader.clone(),
            content_objects.clone(),
            storage_scanner.clone(),
            directories.clone(),
            kind_registry.clone(),
            upload_sessions,
            locks.clone(),
        );
        let actions = ActionOrchestrator::new(
            resources.clone(),
            content.clone(),
            content_reader.clone(),
            action_registry,
            action_executor,
            action_policy,
            edit_policy,
        );
        let maintenance = StorageMaintenanceService::new(
            store,
            read_model,
            maintenance_read_model,
            storage_scanner,
            content_reader,
            blob_health,
            directories,
            directory_provisioning,
            kind_registry,
            locks,
        );
        Self {
            resources,
            content,
            uploads,
            actions,
            maintenance,
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
    pub fn action_orchestrator(&self) -> ActionOrchestrator {
        self.actions.clone()
    }
    pub fn storage_maintenance_service(&self) -> StorageMaintenanceService {
        self.maintenance.clone()
    }
}

// The old broad Resource service test harness encoded the removed facade and soft-delete API.
// Focused recovery and adapter tests live beside the owning relocation/store implementations.
