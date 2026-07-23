use super::*;
use crate::domain::{User, UserId, UserRole};
use async_trait::async_trait;
use std::sync::Mutex;

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
    async fn create(&self, user: &User) -> Result<(), CoreError> {
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
async fn member_has_full_access_only_inside_workspace_subtree() {
    let workspace = ResourceDirectory::from_path("users/alice").unwrap();
    let user = User::new(
        "alice",
        "credential-hash",
        UserRole::Member,
        workspace.clone(),
    )
    .unwrap();
    let actor = AccessContext::member(user.id());
    let service = AuthorizationService::new(Arc::new(Users::with_user(user)));

    for permission in [
        DirectoryPermission::Read,
        DirectoryPermission::Write,
        DirectoryPermission::Full,
    ] {
        assert!(
            service
                .require(&actor, &workspace, permission)
                .await
                .is_ok()
        );
        assert!(
            service
                .require(
                    &actor,
                    &ResourceDirectory::from_path("users/alice/docs").unwrap(),
                    permission,
                )
                .await
                .is_ok()
        );
    }

    for outside in ["", "users", "users/alice2", "shared"] {
        assert!(
            service
                .require(
                    &actor,
                    &ResourceDirectory::from_path(outside).unwrap(),
                    DirectoryPermission::Read,
                )
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn root_workspace_contains_every_user_directory() {
    let user = User::new(
        "root-member",
        "credential-hash",
        UserRole::Member,
        ResourceDirectory::root(),
    )
    .unwrap();
    let actor = AccessContext::member(user.id());
    let service = AuthorizationService::new(Arc::new(Users::with_user(user)));

    assert!(
        service
            .require(
                &actor,
                &ResourceDirectory::from_path("any/directory").unwrap(),
                DirectoryPermission::Full,
            )
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn administrator_access_does_not_depend_on_a_workspace() {
    let service = AuthorizationService::new(Arc::new(Users::default()));
    let actor = AccessContext::administrator(UserId::new());

    assert!(
        service
            .require(
                &actor,
                &ResourceDirectory::from_path("any/directory").unwrap(),
                DirectoryPermission::Full,
            )
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn member_workspace_scope_resolves_and_projects_relative_paths() {
    let user = User::new(
        "alice",
        "credential-hash",
        UserRole::Member,
        ResourceDirectory::from_path("users/alice").unwrap(),
    )
    .unwrap();
    let actor = AccessContext::member(user.id());
    let service = AuthorizationService::new(Arc::new(Users::with_user(user)));
    let scope = service.workspace_scope(&actor).await.unwrap();

    assert_eq!(
        scope.resolve(&ResourceDirectory::root()).unwrap().path(),
        "users/alice"
    );
    assert_eq!(
        scope
            .resolve(&ResourceDirectory::from_path("images/raw").unwrap())
            .unwrap()
            .path(),
        "users/alice/images/raw"
    );
    assert_eq!(
        scope
            .project(&ResourceDirectory::from_path("users/alice").unwrap())
            .unwrap()
            .path(),
        ""
    );
    assert_eq!(
        scope
            .project(&ResourceDirectory::from_path("users/alice/images/raw").unwrap())
            .unwrap()
            .path(),
        "images/raw"
    );
    assert!(
        scope
            .project(&ResourceDirectory::from_path("users/bob").unwrap())
            .is_err()
    );
}

#[tokio::test]
async fn administrator_workspace_scope_is_identity() {
    let service = AuthorizationService::new(Arc::new(Users::default()));
    let scope = service
        .workspace_scope(&AccessContext::administrator(UserId::new()))
        .await
        .unwrap();
    let directory = ResourceDirectory::from_path("images/raw").unwrap();

    assert_eq!(scope.resolve(&directory).unwrap(), directory);
    assert_eq!(scope.project(&directory).unwrap(), directory);
}
