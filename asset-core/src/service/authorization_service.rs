use crate::{
    CoreError,
    domain::{AccessContext, DirectoryGrant, DirectoryPermission, ResourceDirectory, UserId},
    port::{AccessPolicyRepository, UserRepository},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AuthorizationService {
    policies: Arc<dyn AccessPolicyRepository>,
    users: Arc<dyn UserRepository>,
}

impl AuthorizationService {
    pub fn new(policies: Arc<dyn AccessPolicyRepository>, users: Arc<dyn UserRepository>) -> Self {
        Self { policies, users }
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
        let grants = self
            .policies
            .list_applicable_grants(&context.user_id(), directory)
            .await?;
        let effective = grants
            .iter()
            .map(DirectoryGrant::permission)
            .reduce(DirectoryPermission::stronger);
        if effective.is_some_and(|value| value.allows(permission)) {
            Ok(())
        } else {
            Err(CoreError::forbidden(
                permission_action(permission),
                directory.path(),
            ))
        }
    }
    pub async fn grant(
        &self,
        actor: &AccessContext,
        grant: DirectoryGrant,
    ) -> Result<(), CoreError> {
        if !actor.is_administrator() {
            return Err(CoreError::forbidden("grant directory access", ""));
        }
        let user = self
            .users
            .find_by_id(&grant.user_id())
            .await?
            .ok_or_else(|| CoreError::not_found("user", grant.user_id().to_string()))?;
        if grant.directory() == user.workspace_directory()
            && grant.permission() != DirectoryPermission::Full
        {
            return Err(CoreError::conflict(
                "workspace directory must retain full resource permission",
            ));
        }
        self.policies.save_grant(&grant).await
    }
    pub async fn grants_for(
        &self,
        actor: &AccessContext,
        user_id: UserId,
    ) -> Result<Vec<DirectoryGrant>, CoreError> {
        if actor.user_id() != user_id && !actor.is_administrator() {
            return Err(CoreError::forbidden("list grants", ""));
        }
        self.policies.list_grants(&user_id).await
    }
    pub async fn revoke(
        &self,
        actor: &AccessContext,
        user_id: UserId,
        directory: &ResourceDirectory,
    ) -> Result<(), CoreError> {
        if !actor.is_administrator() {
            return Err(CoreError::forbidden("revoke directory access", ""));
        }
        let user = self
            .users
            .find_by_id(&user_id)
            .await?
            .ok_or_else(|| CoreError::not_found("user", user_id.to_string()))?;
        if directory == user.workspace_directory() {
            return Err(CoreError::conflict(
                "workspace directory grant cannot be revoked",
            ));
        }
        self.policies.remove_grant(&user_id, directory).await
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
