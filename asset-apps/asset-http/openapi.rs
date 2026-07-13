use crate::dto::{
    BinaryContent, ChecksumResponse, CreateDirectoryRequest, CreateResourceRequest,
    DirectoryListingResponse, ErrorResponse, ExecuteResourceActionRequest, HealthResponse,
    ResourceActionDefinitionResponse, ResourceActionOutputResponse, ResourceActionsResponse,
    ResourceContentResponse, ResourceDirectoryResponse, ResourceKindResponse,
    ResourceKindsResponse, ResourceMetadataRequest, ResourceMetadataResponse, ResourcePageResponse,
    ResourceReadResponse, ResourceResponse, ResourceSummaryMetadataRequest,
    ResourceSummaryMetadataResponse, ScanStorageErrorResponse, ScanStorageRequest,
    ScanStorageResponse, UpdateResourceRequest, UploadResourceContentStreamQuery,
};
use crate::{auth, handlers};
use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};

struct CookieSecurity;

impl Modify for CookieSecurity {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "cookie_auth",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("asset_hub_session"))),
            );
        }
    }
}

/// asset-http OpenAPI 文档。
#[derive(OpenApi)]
#[openapi(
    paths(
        auth::login,
        auth::logout,
        auth::me,
        auth::create_user,
        auth::list_users,
        auth::update_user_status,
        auth::grant_directory,
        auth::my_directory_grants,
        auth::revoke_directory,
        handlers::health,
        handlers::list_resource_kinds,
        handlers::list_resources,
        handlers::list_directory,
        handlers::create_directory,
        handlers::scan_storage,
        handlers::create_resource,
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
            CreateDirectoryRequest,
            DirectoryListingResponse,
            ErrorResponse,
            ExecuteResourceActionRequest,
            HealthResponse,
            ResourceKindResponse,
            ResourceKindsResponse,
            ResourceActionDefinitionResponse,
            ResourceActionOutputResponse,
            ResourceContentResponse,
            ResourceDirectoryResponse,
            ResourceActionsResponse,
            ResourceMetadataRequest,
            ResourceMetadataResponse,
            ResourcePageResponse,
            ResourceReadResponse,
            ResourceResponse,
            ResourceSummaryMetadataRequest,
            ResourceSummaryMetadataResponse,
            ScanStorageErrorResponse,
            ScanStorageRequest,
            ScanStorageResponse,
            UpdateResourceRequest,
            UploadResourceContentStreamQuery
            ,auth::AuthenticatedUser
            ,auth::Credentials
            ,auth::MeResponse
            ,auth::CreateUserRequest
            ,auth::GrantDirectoryRequest
            ,auth::DirectoryGrantResponse
            ,auth::ManagedUserResponse
            ,auth::UpdateUserStatusRequest
        )
    ),
    modifiers(&CookieSecurity),
    security(("cookie_auth" = [])),
    tags(
        (name = "system", description = "系统状态接口"),
        (name = "resources", description = "资源管理接口")
        ,(name = "authentication", description = "登录、用户和目录授权接口")
    )
)]
pub(crate) struct ApiDoc;
