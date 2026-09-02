use crate::dto::{
    BinaryContent, ChecksumResponse, CreateDirectoryRequest, CreateUploadRequest,
    DirectoryActionDefinitionResponse, DirectoryActionOutputResponse, DirectoryKindResponse,
    DirectoryKindsResponse, DirectoryListingResponse, DirectoryResponse, ErrorResponse,
    ExecuteDirectoryActionRequest, ExecuteResourceActionRequest, HealthComponentResponse,
    HealthResponse, PluginDiagnosticResponse, ResourceActionDefinitionResponse,
    ResourceActionOutputResponse, ResourceContentResponse, ResourceContentStateResponse,
    ResourceEffectiveStateResponse, ResourceKindResponse, ResourceKindsResponse,
    ResourceLifecycleStateResponse, ResourcePageResponse, ResourceResponse, ResourceStateResponse,
    UpdateDirectoryRequest, UpdateResourceRequest, UploadSessionResponse,
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
        handlers::maintenance::health,
        handlers::resource::list_resource_kinds,
        handlers::directory::list_directory_kinds,
        handlers::resource::list_resources,
        handlers::directory::list_directory,
        handlers::directory::create_directory,
        handlers::directory::find_directory,
        handlers::directory::update_directory,
        handlers::directory::delete_directory,
        handlers::directory::execute_directory_action,
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
        handlers::resource::execute_resource_action,
        handlers::resource::delete_resource
    ),
    components(
        schemas(
            ChecksumResponse,
            BinaryContent,
            CreateDirectoryRequest,
            DirectoryListingResponse,
            DirectoryKindResponse,
            DirectoryKindsResponse,
            DirectoryActionDefinitionResponse,
            DirectoryActionOutputResponse,
            UpdateDirectoryRequest,
            ErrorResponse,
            ExecuteResourceActionRequest,
            ExecuteDirectoryActionRequest,
            PluginDiagnosticResponse,
            HealthResponse,
            HealthComponentResponse,
            ResourceKindResponse,
            ResourceKindsResponse,
            ResourceActionDefinitionResponse,
            ResourceActionOutputResponse,
            ResourceContentResponse,
            ResourceContentStateResponse,
            ResourceEffectiveStateResponse,
            ResourceLifecycleStateResponse,
            ResourceStateResponse,
            DirectoryResponse,
            ResourcePageResponse,
            ResourceResponse,
            UpdateResourceRequest,
            CreateUploadRequest
            ,UploadSessionResponse
            ,auth::AuthenticatedUser
            ,auth::Credentials
            ,auth::MeResponse
            ,auth::CreateUserRequest
            ,auth::ManagedUserResponse
            ,auth::UpdateUserStatusRequest
        )
    ),
    modifiers(&CookieSecurity),
    security(("cookie_auth" = [])),
    tags(
        (name = "system", description = "系统状态接口"),
        (name = "resources", description = "资源管理接口"),
        (name = "directories", description = "目录管理接口")
        ,(name = "uploads", description = "断点续传接口")
        ,(name = "authentication", description = "登录和用户管理接口")
    )
)]
pub(crate) struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_keeps_the_public_identity_and_authentication_contract() {
        let document = serde_json::to_value(ApiDoc::openapi()).unwrap();

        assert!(document["paths"].get("/directories/{id}").is_some());
        assert!(document["paths"].get("/resources/{id}/content").is_some());
        assert_eq!(
            document["components"]["securitySchemes"]["cookie_auth"]["name"],
            "asset_hub_session"
        );
        assert!(
            document["components"]["schemas"]["AuthenticatedUser"]["properties"]
                .get("workspace_directory")
                .is_none()
        );
    }

    #[test]
    fn resource_contract_exposes_only_the_unified_state_model() {
        let document = serde_json::to_value(ApiDoc::openapi()).unwrap();
        let schemas = &document["components"]["schemas"];
        let resource = &schemas["ResourceResponse"]["properties"];
        let content = &schemas["ResourceContentResponse"]["properties"];

        assert!(resource.get("state").is_some());
        assert!(resource.get("deleted_at").is_none());
        assert!(content.get("verification_status").is_none());
        assert!(schemas.get("ResourceStateResponse").is_some());
        assert!(schemas.get("ContentVerificationStatusResponse").is_none());
    }
}
