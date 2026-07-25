use crate::dto::{
    BinaryContent, ChecksumResponse, CreateDirectoryRequest, CreateResourceRequest,
    DirectoryListingResponse, DirectoryResponse, ErrorResponse, ExecuteResourceActionRequest,
    HealthComponentResponse, HealthResponse, PluginDiagnosticResponse,
    ResourceActionDefinitionResponse, ResourceActionOutputResponse, ResourceActionsResponse,
    ResourceContentResponse, ResourceKindResponse, ResourceKindsResponse, ResourcePageResponse,
    ResourceReadResponse, ResourceResponse, UpdateResourceRequest,
    UploadResourceContentStreamQuery,
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
        auth::routes::login,
        auth::routes::logout,
        auth::routes::me,
        auth::routes::create_user,
        auth::routes::list_users,
        auth::routes::update_user_status,
        auth::routes::list_security_audit_events,
        handlers::maintenance::health,
        handlers::resource::list_resource_kinds,
        handlers::resource::list_resources,
        handlers::resource::list_directory,
        handlers::resource::create_directory,
        handlers::resource::create_resource,
        handlers::content::upload_resource_content_stream,
        handlers::resource::find_resource,
        handlers::resource::update_resource,
        handlers::content::get_resource_content,
        handlers::content::preview_resource,
        handlers::content::thumbnail_resource,
        handlers::content::read_resource,
        handlers::resource::execute_resource_action,
        handlers::resource::soft_delete_resource,
        handlers::resource::remove_resource
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
            PluginDiagnosticResponse,
            HealthResponse,
            HealthComponentResponse,
            ResourceKindResponse,
            ResourceKindsResponse,
            ResourceActionDefinitionResponse,
            ResourceActionOutputResponse,
            ResourceContentResponse,
            DirectoryResponse,
            ResourceActionsResponse,
            ResourcePageResponse,
            ResourceReadResponse,
            ResourceResponse,
            UpdateResourceRequest,
            UploadResourceContentStreamQuery
            ,auth::AuthenticatedUser
            ,auth::Credentials
            ,auth::MeResponse
            ,auth::CreateUserRequest
            ,crate::audit::SecurityAuditEventResponse
            ,auth::ManagedUserResponse
            ,auth::UpdateUserStatusRequest
        )
    ),
    modifiers(&CookieSecurity),
    security(("cookie_auth" = [])),
    tags(
        (name = "system", description = "系统状态接口"),
        (name = "resources", description = "资源管理接口")
        ,(name = "authentication", description = "登录和用户管理接口")
    )
)]
pub(crate) struct ApiDoc;
