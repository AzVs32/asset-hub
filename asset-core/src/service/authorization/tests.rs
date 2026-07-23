use super::*;
use crate::domain::{Directory, DirectoryId, DirectoryPath, DirectoryRef, User, UserId, UserRole};
use crate::port::{DirectoryRepository, DirectoryStorage};
use async_trait::async_trait;
use std::{collections::HashMap, sync::Mutex};

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

struct Directories {
    paths: HashMap<DirectoryId, DirectoryPath>,
}

impl Directories {
    fn new(paths: &[&str]) -> Self {
        let mut values = HashMap::from([(DirectoryId::root(), DirectoryPath::root())]);
        for value in paths {
            let path = DirectoryPath::from_path(*value).unwrap();
            values.insert(DirectoryId::new(), path);
        }
        Self { paths: values }
    }

    fn reference(&self, path: &str) -> DirectoryRef {
        let path = DirectoryPath::from_path(path).unwrap();
        let id = self
            .paths
            .iter()
            .find_map(|(id, candidate)| (candidate == &path).then_some(*id))
            .unwrap_or_else(|| {
                if path.is_root() {
                    DirectoryId::root()
                } else {
                    DirectoryId::new()
                }
            });
        DirectoryRef::new(id, path)
    }
}

#[async_trait]
impl DirectoryStorage for Directories {
    async fn ensure_directory(&self, _directory: &DirectoryPath) -> Result<(), CoreError> {
        Ok(())
    }
}

#[async_trait]
impl DirectoryRepository for Directories {
    async fn save_directory(&self, _directory: &Directory) -> Result<(), CoreError> {
        Ok(())
    }
    async fn find_directory(&self, _id: &DirectoryId) -> Result<Option<Directory>, CoreError> {
        Ok(None)
    }
    async fn locate_by_id(&self, id: &DirectoryId) -> Result<Option<DirectoryRef>, CoreError> {
        Ok(self
            .paths
            .get(id)
            .cloned()
            .map(|path| DirectoryRef::new(*id, path)))
    }
    async fn locate_by_path(
        &self,
        path: &DirectoryPath,
    ) -> Result<Option<DirectoryRef>, CoreError> {
        Ok(self.paths.iter().find_map(|(id, candidate)| {
            (candidate == path).then(|| DirectoryRef::new(*id, candidate.clone()))
        }))
    }
    async fn list_children(
        &self,
        _parent_id: &DirectoryId,
    ) -> Result<Vec<DirectoryRef>, CoreError> {
        Ok(Vec::new())
    }
    async fn ensure_path(&self, path: &DirectoryPath) -> Result<DirectoryRef, CoreError> {
        self.locate_by_path(path)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", path.path()))
    }
    async fn remove_if_empty(&self, _id: &DirectoryId) -> Result<bool, CoreError> {
        Ok(false)
    }
    async fn is_descendant_or_self(
        &self,
        ancestor_id: &DirectoryId,
        candidate_id: &DirectoryId,
    ) -> Result<bool, CoreError> {
        let ancestor = self.paths.get(ancestor_id);
        let candidate = self.paths.get(candidate_id);
        Ok(
            matches!((ancestor, candidate), (Some(ancestor), Some(candidate)) if ancestor.contains(candidate)),
        )
    }
}

fn authorization(users: Users, directories: Arc<Directories>) -> AuthorizationService {
    AuthorizationService::new(
        Arc::new(users),
        DirectoryService::new(directories.clone(), directories),
    )
}

#[tokio::test]
async fn member_has_full_access_only_inside_workspace_subtree() {
    let directories = Arc::new(Directories::new(&[
        "users",
        "users/alice",
        "users/alice/docs",
        "users/alice2",
        "shared",
    ]));
    let workspace = directories.reference("users/alice");
    let user = User::new(
        "alice",
        "credential-hash",
        UserRole::Member,
        workspace.clone(),
    )
    .unwrap();
    let actor = AccessContext::member(user.id());
    let service = authorization(Users::with_user(user), directories.clone());

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
                    &directories.reference("users/alice/docs"),
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
                    &directories.reference(outside),
                    DirectoryPermission::Read,
                )
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn root_workspace_contains_every_user_directory() {
    let directories = Arc::new(Directories::new(&["any/directory"]));
    let user = User::new(
        "root-member",
        "credential-hash",
        UserRole::Member,
        DirectoryRef::root(),
    )
    .unwrap();
    let actor = AccessContext::member(user.id());
    let service = authorization(Users::with_user(user), directories.clone());

    assert!(
        service
            .require(
                &actor,
                &directories.reference("any/directory"),
                DirectoryPermission::Full,
            )
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn administrator_access_does_not_depend_on_a_workspace() {
    let directories = Arc::new(Directories::new(&["any/directory"]));
    let service = authorization(Users::default(), directories.clone());
    let actor = AccessContext::administrator(UserId::new());

    assert!(
        service
            .require(
                &actor,
                &directories.reference("any/directory"),
                DirectoryPermission::Full,
            )
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn member_workspace_scope_resolves_and_projects_relative_paths() {
    let directories = Arc::new(Directories::new(&["users/alice"]));
    let user = User::new(
        "alice",
        "credential-hash",
        UserRole::Member,
        directories.reference("users/alice"),
    )
    .unwrap();
    let actor = AccessContext::member(user.id());
    let service = authorization(Users::with_user(user), directories);
    let scope = service.workspace_scope(&actor).await.unwrap();

    assert_eq!(
        scope.resolve(&DirectoryPath::root()).unwrap().path(),
        "users/alice"
    );
    assert_eq!(
        scope
            .resolve(&DirectoryPath::from_path("images/raw").unwrap())
            .unwrap()
            .path(),
        "users/alice/images/raw"
    );
    assert_eq!(
        scope
            .project(&DirectoryPath::from_path("users/alice").unwrap())
            .unwrap()
            .path(),
        ""
    );
    assert_eq!(
        scope
            .project(&DirectoryPath::from_path("users/alice/images/raw").unwrap())
            .unwrap()
            .path(),
        "images/raw"
    );
    assert!(
        scope
            .project(&DirectoryPath::from_path("users/bob").unwrap())
            .is_err()
    );
}

#[tokio::test]
async fn administrator_workspace_scope_is_identity() {
    let directories = Arc::new(Directories::new(&[]));
    let service = authorization(Users::default(), directories);
    let scope = service
        .workspace_scope(&AccessContext::administrator(UserId::new()))
        .await
        .unwrap();
    let directory = DirectoryPath::from_path("images/raw").unwrap();

    assert_eq!(scope.resolve(&directory).unwrap(), directory);
    assert_eq!(scope.project(&directory).unwrap(), directory);
}
