use crate::{
    CoreError,
    domain::{AccessContext, DirectoryPermission, ResourceDirectory},
    port::UserRepository,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AuthorizationService {
    users: Arc<dyn UserRepository>,
}

impl AuthorizationService {
    pub fn new(users: Arc<dyn UserRepository>) -> Self {
        Self { users }
    }
    pub async fn require(
        &self,
        context: &AccessContext,
        directory: &ResourceDirectory,
        permission: DirectoryPermission,
    ) -> Result<(), CoreError> {
        if context.is_administrator() {
            return Ok(());
        }
        let user = self
            .users
            .find_by_id(&context.user_id())
            .await?
            .ok_or_else(|| CoreError::not_found("user", context.user_id().to_string()))?;
        if user.workspace_directory().contains(directory) {
            Ok(())
        } else {
            Err(CoreError::forbidden(
                permission_action(permission),
                directory.path(),
            ))
        }
    }
}

fn permission_action(permission: DirectoryPermission) -> &'static str {
    match permission {
        DirectoryPermission::Read => "read",
        DirectoryPermission::Write => "write",
        DirectoryPermission::Full => "full",
    }
}

#[cfg(test)]
mod tests;
