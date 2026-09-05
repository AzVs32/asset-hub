use super::*;
use crate::domain::{Directory, DirectoryId, DirectoryPath, User, UserId, UserRole};
use crate::port::{
    DirectoryIndex, DirectoryLocation, DirectoryQuery, DirectoryRelocation,
    DirectoryRelocationStore, DirectoryRevisionUpdate, DirectoryStorage, DirectoryStore,
    LocatedDirectory,
};
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
}

struct Directories {
    values: Mutex<HashMap<DirectoryId, (Directory, DirectoryPath)>>,
    relocations: Mutex<HashMap<DirectoryId, DirectoryRelocation>>,
}

impl Directories {
    fn new(paths: &[&str]) -> Self {
        let root = Directory::root();
        let mut values = HashMap::from([(root.id(), (root, DirectoryPath::root()))]);
        let mut paths = paths
            .iter()
            .flat_map(|path| {
                let segments = path
                    .split('/')
                    .filter(|segment| !segment.is_empty())
                    .collect::<Vec<_>>();
                (1..=segments.len()).map(move |length| {
                    DirectoryPath::from_path(segments[..length].join("/")).unwrap()
                })
            })
            .collect::<Vec<_>>();
        paths.sort_by(|left, right| left.path().cmp(right.path()));
        paths.dedup();
        paths.sort_by_key(|path| path.path().matches('/').count());
        for path in paths {
            let parent_id = values
                .iter()
                .find_map(|(id, (_, candidate))| {
                    (candidate.path() == path.parent_path()).then_some(*id)
                })
                .unwrap();
            let directory = Directory::new(parent_id, path.name()).unwrap();
            values.insert(directory.id(), (directory, path));
        }
        Self {
            values: Mutex::new(values),
            relocations: Mutex::new(HashMap::new()),
        }
    }

    fn reference(&self, path: &str) -> DirectoryLocation {
        let path = DirectoryPath::from_path(path).unwrap();
        let id = self
            .values
            .lock()
            .unwrap()
            .iter()
            .find_map(|(id, (_, candidate))| (candidate == &path).then_some(*id))
            .unwrap_or_else(|| {
                if path.is_root() {
                    DirectoryId::root()
                } else {
                    DirectoryId::new()
                }
            });
        DirectoryLocation::new(id, path)
    }
}

#[async_trait]
impl DirectoryStorage for Directories {
    async fn directory_exists(&self, _directory: &DirectoryPath) -> Result<bool, CoreError> {
        Ok(true)
    }

    async fn ensure_directory(&self, _directory: &DirectoryPath) -> Result<(), CoreError> {
        Ok(())
    }

    async fn delete_empty_directory(&self, _directory: &DirectoryPath) -> Result<(), CoreError> {
        Ok(())
    }
}

#[async_trait]
impl DirectoryStore for Directories {
    async fn load_all(&self) -> Result<Vec<Directory>, CoreError> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .values()
            .map(|(directory, _)| directory.clone())
            .collect())
    }
    async fn load(&self, id: &DirectoryId) -> Result<Option<Directory>, CoreError> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(id)
            .map(|(directory, _)| directory.clone()))
    }
    async fn insert(&self, _directory: &Directory) -> Result<(), CoreError> {
        Ok(())
    }
    async fn update_batch_if_unchanged(
        &self,
        _updates: &[DirectoryRevisionUpdate],
    ) -> Result<bool, CoreError> {
        Ok(true)
    }
    async fn is_empty(&self, _id: &DirectoryId) -> Result<bool, CoreError> {
        Ok(false)
    }
    async fn delete_if_empty(
        &self,
        _id: &DirectoryId,
        _expected_revision: u64,
    ) -> Result<bool, CoreError> {
        Ok(false)
    }
}

#[async_trait]
impl DirectoryRelocationStore for Directories {
    async fn begin(&self, relocation: &DirectoryRelocation) -> Result<(), CoreError> {
        self.relocations
            .lock()
            .unwrap()
            .insert(relocation.directory_id(), relocation.clone());
        Ok(())
    }

    async fn load_pending(&self) -> Result<Vec<DirectoryRelocation>, CoreError> {
        Ok(self.relocations.lock().unwrap().values().cloned().collect())
    }

    async fn complete(&self, id: &DirectoryId) -> Result<(), CoreError> {
        self.relocations.lock().unwrap().remove(id);
        Ok(())
    }
}

#[async_trait]
impl DirectoryQuery for Directories {
    async fn find_by_id(&self, id: &DirectoryId) -> Result<Option<LocatedDirectory>, CoreError> {
        self.values
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .map(|(directory, path)| {
                LocatedDirectory::new(directory, DirectoryLocation::new(*id, path))
            })
            .transpose()
    }
    async fn find_by_path(
        &self,
        path: &DirectoryPath,
    ) -> Result<Option<LocatedDirectory>, CoreError> {
        self.values
            .lock()
            .unwrap()
            .iter()
            .find(|(_, (_, candidate))| candidate == path)
            .map(|(id, (directory, candidate))| {
                LocatedDirectory::new(
                    directory.clone(),
                    DirectoryLocation::new(*id, candidate.clone()),
                )
            })
            .transpose()
    }
    async fn list_children(
        &self,
        _parent_id: &DirectoryId,
    ) -> Result<Vec<LocatedDirectory>, CoreError> {
        Ok(Vec::new())
    }
    async fn is_descendant_or_self(
        &self,
        ancestor_id: &DirectoryId,
        candidate_id: &DirectoryId,
    ) -> Result<bool, CoreError> {
        let values = self.values.lock().unwrap();
        let ancestor = values.get(ancestor_id).map(|(_, path)| path);
        let candidate = values.get(candidate_id).map(|(_, path)| path);
        Ok(
            matches!((ancestor, candidate), (Some(ancestor), Some(candidate)) if ancestor.contains(candidate)),
        )
    }
}

#[async_trait]
impl DirectoryIndex for Directories {
    async fn replace_all(&self, _directories: Vec<Directory>) -> Result<(), CoreError> {
        Ok(())
    }
    async fn upsert(&self, _directory: Directory) -> Result<(), CoreError> {
        Ok(())
    }
    async fn remove(&self, _id: &DirectoryId) -> Result<(), CoreError> {
        Ok(())
    }
}

fn authorization(users: Users, directories: Arc<Directories>) -> AuthorizationService {
    AuthorizationService::new(
        Arc::new(users),
        crate::service::DirectoryServices::new(
            directories.clone(),
            directories.clone(),
            directories.clone(),
            directories,
        )
        .directory_service(),
    )
}

#[tokio::test]
async fn member_operations_are_allowed_only_inside_workspace_subtree() {
    let directories = Arc::new(Directories::new(&[
        "users",
        "users/alice",
        "users/alice/docs",
        "users/alice2",
        "shared",
    ]));
    let workspace = directories.reference("users/alice");
    let user = User::new("alice", "credential-hash", UserRole::Member, workspace.id()).unwrap();
    let actor = AccessContext::member(user.id());
    let service = authorization(Users::with_user(user), directories.clone());

    for operation in [
        DirectoryOperation::ViewDirectory,
        DirectoryOperation::DownloadDirectory,
        DirectoryOperation::CreateDirectory,
        DirectoryOperation::DeleteDirectory,
        DirectoryOperation::CreateResource,
        DirectoryOperation::ReadResource,
        DirectoryOperation::UpdateResource,
        DirectoryOperation::ReplaceResourceContent,
        DirectoryOperation::DeleteResource,
    ] {
        assert!(service.require(&actor, &workspace, operation).await.is_ok());
        assert!(
            service
                .require(
                    &actor,
                    &directories.reference("users/alice/docs"),
                    operation,
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
                    DirectoryOperation::ViewDirectory,
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
        DirectoryId::root(),
    )
    .unwrap();
    let actor = AccessContext::member(user.id());
    let service = authorization(Users::with_user(user), directories.clone());

    assert!(
        service
            .require(
                &actor,
                &directories.reference("any/directory"),
                DirectoryOperation::DeleteResource,
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
                DirectoryOperation::DeleteResource,
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
        directories.reference("users/alice").id(),
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
