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
    pub(super) workspace_directory: DirectoryPath,
}

impl From<User> for ManagedUserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id(),
            username: user.username().to_owned(),
            role: user.role(),
            status: user.status(),
            workspace_directory: user.workspace_directory().path().clone(),
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
    pub(super) workspace_directory: Option<DirectoryPath>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct UpdateUserStatusRequest {
    #[schema(value_type = String)]
    pub(super) status: UserStatus,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct SecurityAuditQuery {
    pub(super) page: Option<u32>,
    pub(super) limit: Option<u32>,
}
