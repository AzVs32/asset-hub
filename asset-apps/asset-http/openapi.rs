use crate::dto::{
    BinaryContent, ChecksumResponse, CreateResourceRequest, ErrorResponse,
    ExecuteResourceActionRequest, HealthResponse, KindMetadataRequest, KindMetadataResponse,
    ResourceActionDefinitionResponse, ResourceActionOutputResponse, ResourceActionsResponse,
    ResourceContentResponse, ResourceKindResponse, ResourceKindsResponse, ResourceMetadataRequest,
    ResourceMetadataResponse, ResourcePageResponse, ResourceReadResponse, ResourceResponse,
    ResourceSummaryMetadataRequest, ResourceSummaryMetadataResponse, UpdateResourceRequest,
    UploadResourceContentRequest,
};
use crate::handlers;
use utoipa::OpenApi;

/// asset-http OpenAPI 文档。
#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::health,
        handlers::list_resource_kinds,
        handlers::list_resources,
        handlers::create_resource,
        handlers::upload_resource_content,
        handlers::upload_resource_content_stream,
        handlers::find_resource,
        handlers::update_resource,
        handlers::get_resource_content,
        handlers::preview_resource,
        handlers::thumbnail_resource,
        handlers::read_resource,
        handlers::execute_resource_action,
        handlers::soft_delete_resource,
        handlers::remove_resource
    ),
    components(
        schemas(
            ChecksumResponse,
            BinaryContent,
            CreateResourceRequest,
            ErrorResponse,
            ExecuteResourceActionRequest,
            HealthResponse,
            KindMetadataRequest,
            KindMetadataResponse,
            ResourceKindResponse,
            ResourceKindsResponse,
            ResourceActionDefinitionResponse,
            ResourceActionOutputResponse,
            ResourceContentResponse,
            ResourceActionsResponse,
            ResourceMetadataRequest,
            ResourceMetadataResponse,
            ResourcePageResponse,
            ResourceReadResponse,
            ResourceResponse,
            ResourceSummaryMetadataRequest,
            ResourceSummaryMetadataResponse,
            UpdateResourceRequest,
            UploadResourceContentRequest
        )
    ),
    tags(
        (name = "system", description = "系统状态接口"),
        (name = "resources", description = "资源管理接口")
    )
)]
pub(crate) struct ApiDoc;
