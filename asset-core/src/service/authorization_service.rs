use crate::{
    CoreError,
    domain::{AccessContext, DirectoryGrant, DirectoryPermission, UserId, normalize_directory},
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
        directory: &str,
        permission: DirectoryPermission,
    ) -> Result<(), CoreError> {
        let directory = normalize_directory(directory.to_owned())?;
        if context.is_administrator() {
            return Ok(());
        }
        let effective = self
            .repository
            .effective_permission(&context.user_id(), &directory)
            .await?;
        if effective.is_some_and(|value| value >= permission) {
            Ok(())
        } else {
            Err(CoreError::forbidden(
                permission_action(permission),
                directory,
            ))
        }
    }
    pub async fn grant(
        &self,
        actor: &AccessContext,
        grant: DirectoryGrant,
    ) -> Result<(), CoreError> {
        if !actor.is_administrator() {
            self.require(actor, grant.directory(), DirectoryPermission::Manage)
                .await?;
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
        directory: &str,
    ) -> Result<(), CoreError> {
        let directory = normalize_directory(directory.to_owned())?;
        if !actor.is_administrator() {
            self.require(actor, &directory, DirectoryPermission::Manage)
                .await?;
        }
        self.repository.remove_grant(&user_id, &directory).await
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
        async fn remove_grant(&self, user_id: &UserId, directory: &str) -> Result<(), CoreError> {
            self.grants
                .lock()
                .unwrap()
                .retain(|grant| grant.user_id() != *user_id || grant.directory() != directory);
            Ok(())
        }
        async fn effective_permission(
            &self,
            user_id: &UserId,
            directory: &str,
        ) -> Result<Option<DirectoryPermission>, CoreError> {
            let grants = self.grants.lock().unwrap();
            Ok(grants
                .iter()
                .filter(|g| {
                    g.user_id() == *user_id
                        && (g.directory().is_empty()
                            || directory == g.directory()
                            || directory
                                .strip_prefix(g.directory())
                                .is_some_and(|suffix| suffix.starts_with('/')))
                })
                .max_by_key(|g| g.directory().len())
                .map(DirectoryGrant::permission))
        }
    }

    #[tokio::test]
    async fn inherited_permission_obeys_rank_and_boundary() {
        let repository = Arc::new(Policies::default());
        let service = AuthorizationService::new(repository.clone());
        let user = UserId::new();
        let admin = AccessContext::administrator(UserId::new());
        service
            .grant(
                &admin,
                DirectoryGrant::new(user, "teams/alice", DirectoryPermission::Write).unwrap(),
            )
            .await
            .unwrap();
        let actor = AccessContext::user(user);
        assert!(
            service
                .require(&actor, "teams/alice/docs", DirectoryPermission::Read)
                .await
                .is_ok()
        );
        assert!(
            service
                .require(&actor, "teams/alice/docs", DirectoryPermission::Write)
                .await
                .is_ok()
        );
        assert!(
            service
                .require(&actor, "teams/alice/docs", DirectoryPermission::Manage)
                .await
                .is_err()
        );
        assert!(
            service
                .require(&actor, "teams/alice2", DirectoryPermission::Read)
                .await
                .is_err()
        );
    }
}
