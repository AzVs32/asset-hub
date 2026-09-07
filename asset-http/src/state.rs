use asset_core::CoreError;
use asset_core::service::{
    AssetWorkflowService, ContentService, DirectoryService, ResourceService,
    StorageMaintenanceService, UploadService,
};
use asset_runtime::UploadFinalizationDispatcher;
use std::sync::Arc;

/// HTTP routes that operate on Resource aggregates or their content.
///
/// This is a composition-only grouping; application workflows remain on the individual services.
#[derive(Clone)]
pub struct ResourceHttpServices {
    pub resources: ResourceService,
    pub content: ContentService,
    pub uploads: UploadService,
}

/// HTTP routes that address Directory aggregates.
#[derive(Clone)]
pub struct DirectoryHttpServices {
    pub directories: DirectoryService,
}

/// The sole maintenance interface required by the public HTTP surface.
///
/// It supports `/health` Blob readiness only. Recovery and reconciliation operations are not
/// exposed to handlers or added as HTTP endpoints.
#[derive(Clone)]
pub struct HttpHealthServices {
    pub storage_maintenance: StorageMaintenanceService,
}

/// Application services consumed by HTTP routes, grouped only by transport dependency shape.
///
/// The bundle owns no business methods and does not merge the constituent service boundaries.
#[derive(Clone)]
pub struct HttpServices {
    pub resources: ResourceHttpServices,
    pub directories: DirectoryHttpServices,
    pub workflows: AssetWorkflowService,
    pub health: HttpHealthServices,
}

/// All dependencies needed to compose the HTTP router once at application startup.
///
/// Unlike [`HttpState`], this is public because executable composition happens outside the library.
pub struct HttpComposition {
    pub services: HttpServices,
    pub upload_finalizations: Arc<dyn UploadFinalizationDispatcher>,
}

/// HTTP handler shared state.
///
/// Axum clones this state for each request. The bundled services already own their shared ports,
/// so cloning retains the existing ownership model without introducing `Arc<Arc<Service>>`.
#[derive(Clone)]
pub(crate) struct HttpState {
    services: HttpServices,
    upload_finalizations: Arc<dyn UploadFinalizationDispatcher>,
}

impl HttpState {
    pub(crate) fn new(composition: HttpComposition) -> Self {
        Self {
            services: composition.services,
            upload_finalizations: composition.upload_finalizations,
        }
    }

    pub(crate) fn dispatch_upload_finalization(
        &self,
        id: asset_core::domain::UploadId,
    ) -> Result<(), CoreError> {
        self.upload_finalizations.dispatch(id)
    }

    pub(crate) fn resources(&self) -> &ResourceService {
        &self.services.resources.resources
    }

    pub(crate) fn directories(&self) -> &DirectoryService {
        &self.services.directories.directories
    }

    pub(crate) fn content(&self) -> &ContentService {
        &self.services.resources.content
    }

    pub(crate) fn workflows(&self) -> &AssetWorkflowService {
        &self.services.workflows
    }

    /// Upload sessions are exposed through this direct Core application service.
    pub(crate) fn uploads(&self) -> &UploadService {
        &self.services.resources.uploads
    }

    pub(crate) async fn check_blob_storage_health(&self) -> Result<(), CoreError> {
        self.services
            .health
            .storage_maintenance
            .check_blob_storage_health()
            .await
    }
}
