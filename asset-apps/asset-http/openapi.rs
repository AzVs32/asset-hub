use crate::dto::{
    BinaryContent, ChecksumResponse, CreateResourceRequest, ErrorResponse, HealthResponse,
    ResourceContentResponse, ResourceMetadataRequest, ResourceMetadataResponse, ResourceResponse,
    UploadResourceContentRequest,
};
use crate::handlers;
use utoipa::OpenApi;

/// asset-http OpenAPI 文档。
#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::health,
        handlers::create_resource,
        handlers::upload_resource_content,
        handlers::upload_resource_content_stream,
        handlers::find_resource,
        handlers::get_resource_content,
        handlers::soft_delete_resource,
        handlers::remove_resource
    ),
    components(
        schemas(
            ChecksumResponse,
            BinaryContent,
            CreateResourceRequest,
            ErrorResponse,
            HealthResponse,
            ResourceContentResponse,
            ResourceMetadataRequest,
            ResourceMetadataResponse,
            ResourceResponse,
            UploadResourceContentRequest
        )
    ),
    tags(
        (name = "system", description = "系统状态接口"),
        (name = "resources", description = "资源管理接口")
    )
)]
pub(crate) struct ApiDoc;
