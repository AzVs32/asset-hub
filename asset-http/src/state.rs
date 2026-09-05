use asset_core::CoreError;
use asset_core::domain::AccessContext;
use asset_core::service::{
    AssetWorkflowService, AuthorizationService, ContentService, DirectoryService, ResourceService,
    SecuredAssetWorkflowService, SecuredContentService, SecuredDirectoryService,
    SecuredResourceService, SecuredUploadService, StorageMaintenanceService, UploadService,
    WorkspaceScope,
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
    pub authorization: AuthorizationService,
    pub upload_finalizations: Arc<dyn UploadFinalizationDispatcher>,
}

/// HTTP handler shared state.
///
/// Axum clones this state for each request. The bundled services already own their shared ports,
/// so cloning retains the existing ownership model without introducing `Arc<Arc<Service>>`.
#[derive(Clone)]
pub(crate) struct HttpState {
    services: HttpServices,
    authorization: AuthorizationService,
    upload_finalizations: Arc<dyn UploadFinalizationDispatcher>,
}

impl HttpState {
    pub(crate) fn new(composition: HttpComposition) -> Self {
        Self {
            services: composition.services,
            authorization: composition.authorization,
            upload_finalizations: composition.upload_finalizations,
        }
    }

    pub(crate) fn secured_resources<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredResourceService<'a> {
        self.services
            .resources
            .resources
            .secured(&self.authorization, context)
    }

    pub(crate) fn secured_directories<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredDirectoryService<'a> {
        self.services
            .directories
            .directories
            .secured(&self.authorization, context)
    }

    pub(crate) fn secured_content<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredContentService<'a> {
        self.services
            .resources
            .content
            .secured(&self.authorization, context)
    }

    pub(crate) fn secured_uploads<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredUploadService<'a> {
        self.services
            .resources
            .uploads
            .secured(&self.authorization, context)
    }

    pub(crate) fn secured_asset_coordination<'a>(
        &'a self,
        context: &'a AccessContext,
    ) -> SecuredAssetWorkflowService<'a> {
        self.services
            .workflows
            .secured(&self.authorization, context)
    }

    pub(crate) fn dispatch_upload_finalization(
        &self,
        id: asset_core::domain::UploadId,
    ) -> Result<(), CoreError> {
        self.upload_finalizations.dispatch(id)
    }

    pub(crate) async fn workspace(
        &self,
        context: &AccessContext,
    ) -> Result<WorkspaceScope, CoreError> {
        self.authorization.workspace_scope(context).await
    }

    pub(crate) fn resources(&self) -> &ResourceService {
        &self.services.resources.resources
    }

    pub(crate) async fn check_blob_storage_health(&self) -> Result<(), CoreError> {
        self.services
            .health
            .storage_maintenance
            .check_blob_storage_health()
            .await
    }
}
