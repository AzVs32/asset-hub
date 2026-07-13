use crate::{
    CoreError,
    domain::{AccessContext, DirectoryGrant, DirectoryPermission, ResourceDirectory, UserId},
    port::AccessPolicyRepository,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AuthorizationService {
    repository: Arc<dyn AccessPolicyRepository>,
}

impl AuthorizationService {
    pub fn new(repository: Arc<dyn AccessPolicyRepository>) -> Self {
        Self { repository }
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
        if context.home_directory().contains(directory) {
            return Ok(());
        }
        let effective = self
            .repository
            .effective_permission(&context.user_id(), directory)
            .await?;
        if effective.is_some_and(|value| value >= permission) {
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
        self.repository.save_grant(&grant).await
    }
    pub async fn grants_for(
        &self,
        actor: &AccessContext,
        user_id: UserId,
    ) -> Result<Vec<DirectoryGrant>, CoreError> {
        if actor.user_id() != user_id && !actor.is_administrator() {
            return Err(CoreError::forbidden("list grants", ""));
        }
        self.repository.list_grants(&user_id).await
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
        self.repository.remove_grant(&user_id, directory).await
    }
}

fn permission_action(permission: DirectoryPermission) -> &'static str {
    match permission {
        DirectoryPermission::Read => "read",
        DirectoryPermission::Write => "write",
        DirectoryPermission::Manage => "manage",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Policies {
        grants: Mutex<Vec<DirectoryGrant>>,
    }
    #[async_trait]
    impl AccessPolicyRepository for Policies {
        async fn save_grant(&self, grant: &DirectoryGrant) -> Result<(), CoreError> {
            self.grants.lock().unwrap().push(grant.clone());
            Ok(())
        }
        async fn list_grants(&self, user_id: &UserId) -> Result<Vec<DirectoryGrant>, CoreError> {
            Ok(self
                .grants
                .lock()
                .unwrap()
                .iter()
                .filter(|g| g.user_id() == *user_id)
                .cloned()
                .collect())
        }
        async fn remove_grant(
            &self,
            user_id: &UserId,
            directory: &ResourceDirectory,
        ) -> Result<(), CoreError> {
            self.grants
                .lock()
                .unwrap()
                .retain(|grant| grant.user_id() != *user_id || grant.directory() != directory);
            Ok(())
        }
        async fn effective_permission(
            &self,
            user_id: &UserId,
            directory: &ResourceDirectory,
        ) -> Result<Option<DirectoryPermission>, CoreError> {
            let grants = self.grants.lock().unwrap();
            Ok(grants
                .iter()
                .filter(|grant| {
                    grant.user_id() == *user_id && grant.directory().contains(directory)
                })
                .map(DirectoryGrant::permission)
                .max())
        }
    }

    #[tokio::test]
    async fn home_and_additional_grants_obey_rank_and_boundary() {
        let repository = Arc::new(Policies::default());
        let service = AuthorizationService::new(repository.clone());
        let user = UserId::new();
        let admin = AccessContext::administrator(UserId::new());
        let shared = ResourceDirectory::from_path("shared").unwrap();
        service
            .grant(
                &admin,
                DirectoryGrant::new(user, shared.clone(), DirectoryPermission::Write),
            )
            .await
            .unwrap();
        service
            .grant(
                &admin,
                DirectoryGrant::new(
                    user,
                    ResourceDirectory::from_path("shared/photos").unwrap(),
                    DirectoryPermission::Read,
                ),
            )
            .await
            .unwrap();
        let home = ResourceDirectory::from_path("users/alice").unwrap();
        let actor = AccessContext::member(user, home.clone());
        assert!(
            service
                .require(
                    &actor,
                    &ResourceDirectory::from_path("users/alice/docs").unwrap(),
                    DirectoryPermission::Manage,
                )
                .await
                .is_ok()
        );
        assert!(
            service
                .require(
                    &actor,
                    &ResourceDirectory::from_path("shared/photos/raw").unwrap(),
                    DirectoryPermission::Write,
                )
                .await
                .is_ok()
        );
        assert!(
            service
                .require(
                    &actor,
                    &ResourceDirectory::from_path("shared/photos/raw").unwrap(),
                    DirectoryPermission::Manage,
                )
                .await
                .is_err()
        );
        assert!(
            service
                .require(
                    &actor,
                    &ResourceDirectory::from_path("users/alice2").unwrap(),
                    DirectoryPermission::Read,
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn only_administrators_can_change_grants() {
        let service = AuthorizationService::new(Arc::new(Policies::default()));
        let user = UserId::new();
        let member =
            AccessContext::member(user, ResourceDirectory::from_path("users/alice").unwrap());
        let grant = DirectoryGrant::new(
            UserId::new(),
            ResourceDirectory::from_path("users/alice/shared").unwrap(),
            DirectoryPermission::Read,
        );

        assert!(service.grant(&member, grant).await.is_err());
    }
}
