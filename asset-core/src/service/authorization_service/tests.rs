use super::*;
use crate::domain::{User, UserRole};
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
    async fn list_applicable_grants(
        &self,
        user_id: &UserId,
        directory: &ResourceDirectory,
    ) -> Result<Vec<DirectoryGrant>, CoreError> {
        let grants = self.grants.lock().unwrap();
        Ok(grants
            .iter()
            .filter(|grant| grant.user_id() == *user_id && grant.directory().contains(directory))
            .cloned()
            .collect())
    }
}

#[derive(Default)]
struct Users {
    users: Mutex<Vec<User>>,
}

impl Users {
    fn with_user(user: User) -> Self {
        Self {
            users: Mutex::new(vec![user]),
        }
    }
}

#[async_trait]
impl UserRepository for Users {
    async fn create(
        &self,
        user: &User,
        _workspace_grant: &DirectoryGrant,
    ) -> Result<(), CoreError> {
        self.users.lock().unwrap().push(user.clone());
        Ok(())
    }

    async fn save(&self, user: &User) -> Result<(), CoreError> {
        let mut users = self.users.lock().unwrap();
        if let Some(saved) = users.iter_mut().find(|saved| saved.id() == user.id()) {
            *saved = user.clone();
        }
        Ok(())
    }

    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|user| user.id() == *id)
            .cloned())
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|user| user.username() == username)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<User>, CoreError> {
        Ok(self.users.lock().unwrap().clone())
    }

    async fn count(&self) -> Result<u64, CoreError> {
        Ok(self.users.lock().unwrap().len() as u64)
    }
}

#[tokio::test]
async fn explicit_grants_obey_capabilities_and_boundaries() {
    let repository = Arc::new(Policies::default());
    let admin = AccessContext::administrator(UserId::new());
    let shared = ResourceDirectory::from_path("shared").unwrap();
    let workspace = ResourceDirectory::from_path("users/alice").unwrap();
    let target = User::new(
        "alice",
        "credential-hash",
        UserRole::Member,
        workspace.clone(),
    )
    .unwrap();
    let user = target.id();
    let service = AuthorizationService::new(repository.clone(), Arc::new(Users::with_user(target)));
    service
        .grant(
            &admin,
            DirectoryGrant::new(user, workspace.clone(), DirectoryPermission::Full),
        )
        .await
        .unwrap();
    let downgrade_workspace =
        DirectoryGrant::new(user, workspace.clone(), DirectoryPermission::Read);
    assert!(service.grant(&admin, downgrade_workspace).await.is_err());
    assert!(service.revoke(&admin, user, &workspace).await.is_err());
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
    let actor = AccessContext::member(user);
    assert!(
        service
            .require(
                &actor,
                &ResourceDirectory::from_path("users/alice/docs").unwrap(),
                DirectoryPermission::Full,
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
                DirectoryPermission::Full,
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
    let service =
        AuthorizationService::new(Arc::new(Policies::default()), Arc::new(Users::default()));
    let user = UserId::new();
    let member = AccessContext::member(user);
    let grant = DirectoryGrant::new(
        UserId::new(),
        ResourceDirectory::from_path("users/alice/shared").unwrap(),
        DirectoryPermission::Read,
    );

    assert!(service.grant(&member, grant).await.is_err());
}

#[tokio::test]
async fn member_root_access_requires_an_explicit_grant() {
    let repository = Arc::new(Policies::default());
    let service = AuthorizationService::new(repository.clone(), Arc::new(Users::default()));
    let user = UserId::new();
    let member = AccessContext::member(user);
    let root = ResourceDirectory::root();

    assert!(
        service
            .require(&member, &root, DirectoryPermission::Read)
            .await
            .is_err()
    );

    repository
        .save_grant(&DirectoryGrant::new(
            user,
            root.clone(),
            DirectoryPermission::Full,
        ))
        .await
        .unwrap();
    assert!(
        service
            .require(&member, &root, DirectoryPermission::Full)
            .await
            .is_ok()
    );
}
