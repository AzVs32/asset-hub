use crate::dto::{
    BinaryContent, ChecksumResponse, CreateDirectoryRequest, CreateUploadRequest,
    DirectoryListingResponse, DirectoryResponse, ErrorResponse, HealthComponentResponse,
    HealthResponse, ResourceContentResponse, ResourceContentStateResponse,
    ResourceEffectiveStateResponse, ResourceLifecycleStateResponse, ResourcePageResponse,
    ResourceResponse, ResourceStateResponse, UpdateDirectoryRequest, UpdateResourceRequest,
    UploadSessionResponse,
};
use crate::handlers;
use utoipa::OpenApi;

/// asset-http OpenAPI 文档。
#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::maintenance::health,
        handlers::resource::list_resources,
        handlers::directory::list_directory,
        handlers::directory::create_directory,
        handlers::directory::find_directory,
        handlers::directory::update_directory,
        handlers::directory::delete_directory,
        handlers::upload::create_upload,
        handlers::upload::upload_status,
        handlers::upload::append_upload,
        handlers::upload::complete_upload,
        handlers::upload::abort_upload,
        handlers::resource::find_resource,
        handlers::resource::update_resource,
        handlers::content::get_resource_content,
        handlers::content::replace_resource_content,
        handlers::content::download_resource_content,
        handlers::content::download_directory,
        handlers::resource::delete_resource
    ),
    components(
        schemas(
            ChecksumResponse,
            BinaryContent,
            CreateDirectoryRequest,
            DirectoryListingResponse,
            UpdateDirectoryRequest,
            ErrorResponse,
            HealthResponse,
            HealthComponentResponse,
            ResourceContentResponse,
            ResourceContentStateResponse,
            ResourceEffectiveStateResponse,
            ResourceLifecycleStateResponse,
            ResourceStateResponse,
            DirectoryResponse,
            ResourcePageResponse,
            ResourceResponse,
            UpdateResourceRequest,
            CreateUploadRequest,
            UploadSessionResponse
        )
    ),
    tags(
        (name = "system", description = "系统状态接口"),
        (name = "resources", description = "资源管理接口"),
        (name = "directories", description = "目录管理接口"),
        (name = "uploads", description = "断点续传接口")
    )
)]
pub(crate) struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_exposes_only_direct_asset_operations() {
        let document = serde_json::to_value(ApiDoc::openapi()).unwrap();

        assert!(document["paths"].get("/directories/{id}").is_some());
        assert!(document["paths"].get("/resources/{id}/content").is_some());
    }

    #[test]
    fn resource_contract_exposes_only_the_unified_state_model() {
        let document = serde_json::to_value(ApiDoc::openapi()).unwrap();
        let schemas = &document["components"]["schemas"];
        let resource = &schemas["ResourceResponse"]["properties"];
        let content = &schemas["ResourceContentResponse"]["properties"];

        assert!(resource.get("state").is_some());
        assert!(content.get("verification_status").is_none());
        assert!(schemas.get("ResourceStateResponse").is_some());
        assert!(schemas.get("ContentVerificationStatusResponse").is_none());
    }
}
