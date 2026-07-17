use super::*;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub(crate) struct MeResponse {
    pub(super) user: AuthenticatedUser,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ManagedUserResponse {
    #[schema(value_type = String)]
    pub(super) id: UserId,
    pub(super) username: String,
    #[schema(value_type = String)]
    pub(super) role: UserRole,
    #[schema(value_type = String)]
    pub(super) status: UserStatus,
    #[schema(value_type = String)]
    pub(super) workspace_directory: ResourceDirectory,
}

impl From<User> for ManagedUserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id(),
            username: user.username().to_owned(),
            role: user.role(),
            status: user.status(),
            workspace_directory: user.workspace_directory().clone(),
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CreateUserRequest {
    pub(super) username: String,
    pub(super) password: String,
    #[serde(default)]
    pub(super) is_admin: bool,
    #[schema(value_type = Option<String>)]
    pub(super) workspace_directory: Option<ResourceDirectory>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct UpdateUserStatusRequest {
    #[schema(value_type = String)]
    pub(super) status: UserStatus,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct GrantDirectoryRequest {
    #[schema(value_type = String)]
    pub(super) user_id: UserId,
    #[schema(value_type = String)]
    pub(super) directory: ResourceDirectory,
    #[schema(value_type = String)]
    pub(super) permission: DirectoryPermission,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct DirectoryGrantResponse {
    #[schema(value_type = String)]
    pub(super) directory: ResourceDirectory,
    #[schema(value_type = String)]
    pub(super) permission: DirectoryPermission,
    pub(super) is_workspace: bool,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct DirectoryGrantQuery {
    #[param(value_type = Option<String>)]
    pub(super) user_id: Option<UserId>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct RevokeDirectoryGrantQuery {
    #[param(value_type = String)]
    pub(super) user_id: UserId,
    #[param(value_type = String)]
    pub(super) directory: ResourceDirectory,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct SecurityAuditQuery {
    pub(super) page: Option<u32>,
    pub(super) limit: Option<u32>,
}
