use crate::{
    CoreError, UserError,
    domain::{DirectoryPath, User, UserId, UserRole, UserStatus},
    port::{LocatedUser, PasswordHasher, UserQuery, UserRepository},
    service::DirectoryService,
};
use std::sync::Arc;

const MIN_PASSWORD_LEN: usize = 4;

#[derive(Clone)]
pub struct UserService {
    repository: Arc<dyn UserRepository>,
    query: Arc<dyn UserQuery>,
    password_hasher: Arc<dyn PasswordHasher>,
    directories: DirectoryService,
}

impl UserService {
    pub fn new(
        repository: Arc<dyn UserRepository>,
        query: Arc<dyn UserQuery>,
        password_hasher: Arc<dyn PasswordHasher>,
        directories: DirectoryService,
    ) -> Self {
        Self {
            repository,
            query,
            password_hasher,
            directories,
        }
    }
    pub async fn create(
        &self,
        username: impl Into<String>,
        password: &str,
        role: UserRole,
        workspace_directory: Option<DirectoryPath>,
    ) -> Result<User, CoreError> {
        if password.len() < MIN_PASSWORD_LEN {
            return Err(UserError::WeakPassword.into());
        }
        let username = username.into();
        if self
            .repository
            .find_by_username(username.trim())
            .await?
            .is_some()
        {
            return Err(CoreError::conflict("username already exists"));
        }
        let workspace_path = match workspace_directory {
            Some(directory) => directory,
            None if role == UserRole::Administrator => DirectoryPath::root(),
            None => DirectoryPath::from_path(format!("users/{}", username.trim()))?,
        };
        let workspace_directory = self.directories.ensure_path(&workspace_path).await?;
        let user = User::new(
            username,
            self.password_hasher.hash(password)?,
            role,
            workspace_directory.id(),
        )?;
        self.repository.create(&user).await?;
        Ok(user)
    }
    pub async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<User>, CoreError> {
        let Some(user) = self.repository.find_by_username(username.trim()).await? else {
            return Ok(None);
        };
        if !user.is_active()
            || !self
                .password_hasher
                .verify(password, user.credential_hash())?
        {
            return Ok(None);
        }
        Ok(Some(user))
    }
    pub async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError> {
        self.repository.find_by_id(id).await
    }
    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError> {
        self.repository.find_by_username(username.trim()).await
    }
    pub async fn find_located_by_username(
        &self,
        username: &str,
    ) -> Result<Option<LocatedUser>, CoreError> {
        self.query.find_located_by_username(username.trim()).await
    }
    pub async fn list(&self) -> Result<Vec<LocatedUser>, CoreError> {
        self.query.list_located().await
    }
    #[cfg(test)]
    async fn workspace_location(
        &self,
        user: &User,
    ) -> Result<crate::port::DirectoryLocation, CoreError> {
        self.directories
            .locate_by_id(&user.workspace_directory_id())
            .await
    }
    pub async fn update_status(
        &self,
        id: &UserId,
        status: UserStatus,
    ) -> Result<Option<LocatedUser>, CoreError> {
        let Some(located) = self.query.find_located_by_id(id).await? else {
            return Ok(None);
        };
        let (mut user, workspace) = located.into_parts();
        user.change_status(status);
        self.repository.save(&user).await?;
        LocatedUser::new(user, workspace).map(Some)
    }
    pub async fn update_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<User>, CoreError> {
        if password.len() < MIN_PASSWORD_LEN {
            return Err(UserError::WeakPassword.into());
        }
        let Some(mut user) = self.repository.find_by_username(username.trim()).await? else {
            return Ok(None);
        };
        user.change_credential_hash(self.password_hasher.hash(password)?)?;
        self.repository.save(&user).await?;
        Ok(Some(user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Directory, DirectoryId, DirectoryKind};
    use crate::port::{
        DirectoryIndex, DirectoryKindDefinition, DirectoryKindRegistry, DirectoryLocation,
        DirectoryQuery, DirectoryStorage, DirectoryStore, LocatedDirectory, LocatedUser,
        PasswordHasher, UserQuery, UserRepository,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct Users {
        users: Mutex<Vec<User>>,
    }

    #[async_trait]
    impl UserRepository for Users {
        async fn create(&self, user: &User) -> Result<(), CoreError> {
            self.users.lock().unwrap().push(user.clone());
            Ok(())
        }

        async fn save(&self, user: &User) -> Result<(), CoreError> {
            let mut users = self.users.lock().unwrap();
            let saved = users
                .iter_mut()
                .find(|saved| saved.id() == user.id())
                .unwrap();
            *saved = user.clone();
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

    #[async_trait]
    impl UserQuery for Users {
        async fn find_located_by_id(&self, id: &UserId) -> Result<Option<LocatedUser>, CoreError> {
            self.find_by_id(id)
                .await?
                .map(|user| LocatedUser::new(user, DirectoryLocation::root()))
                .transpose()
        }

        async fn find_located_by_username(
            &self,
            username: &str,
        ) -> Result<Option<LocatedUser>, CoreError> {
            self.find_by_username(username)
                .await?
                .map(|user| LocatedUser::new(user, DirectoryLocation::root()))
                .transpose()
        }

        async fn list_located(&self) -> Result<Vec<LocatedUser>, CoreError> {
            self.users
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .map(|user| LocatedUser::new(user, DirectoryLocation::root()))
                .collect()
        }
    }

    struct TestPasswordHasher;

    impl PasswordHasher for TestPasswordHasher {
        fn hash(&self, password: &str) -> Result<String, CoreError> {
            Ok(format!("hashed:{password}"))
        }

        fn verify(&self, password: &str, hash: &str) -> Result<bool, CoreError> {
            Ok(hash == format!("hashed:{password}"))
        }
    }

    struct Directories {
        values: Mutex<HashMap<DirectoryId, (Directory, DirectoryPath)>>,
    }

    impl Default for Directories {
        fn default() -> Self {
            let root = Directory::root();
            Self {
                values: Mutex::new(HashMap::from([(root.id(), (root, DirectoryPath::root()))])),
            }
        }
    }

    #[async_trait]
    impl DirectoryStorage for Directories {
        async fn ensure_directory(&self, _directory: &DirectoryPath) -> Result<(), CoreError> {
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
        async fn insert(&self, directory: &Directory) -> Result<(), CoreError> {
            let parent_id = directory.parent_id().unwrap();
            let parent_path = self
                .values
                .lock()
                .unwrap()
                .get(&parent_id)
                .unwrap()
                .1
                .clone();
            let path = parent_path.child(directory.name())?;
            self.values
                .lock()
                .unwrap()
                .insert(directory.id(), (directory.clone(), path));
            Ok(())
        }
        async fn save_if_unchanged(
            &self,
            directory: &Directory,
            expected_updated_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, CoreError> {
            let current = self.values.lock().unwrap().get(&directory.id()).cloned();
            if !current.is_some_and(|(current, _)| current.updated_at() == expected_updated_at) {
                return Ok(false);
            }
            self.insert(directory).await?;
            Ok(true)
        }
        async fn remove_if_empty(&self, _id: &DirectoryId) -> Result<bool, CoreError> {
            Ok(false)
        }
    }

    #[async_trait]
    impl DirectoryQuery for Directories {
        async fn find_by_id(
            &self,
            id: &DirectoryId,
        ) -> Result<Option<LocatedDirectory>, CoreError> {
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
            Ok(ancestor_id == candidate_id)
        }
    }

    #[async_trait]
    impl DirectoryIndex for Directories {
        async fn replace_all(&self, _directories: Vec<Directory>) -> Result<(), CoreError> {
            Ok(())
        }
        async fn upsert(&self, directory: Directory) -> Result<(), CoreError> {
            self.insert(&directory).await
        }
        async fn remove(&self, id: &DirectoryId) -> Result<(), CoreError> {
            self.values.lock().unwrap().remove(id);
            Ok(())
        }
    }

    struct DirectoryKinds(Vec<DirectoryKindDefinition>);

    impl Default for DirectoryKinds {
        fn default() -> Self {
            Self(vec![DirectoryKindDefinition::with_source(
                DirectoryKind::default(),
                "Directory",
                "test",
            )])
        }
    }

    impl DirectoryKindRegistry for DirectoryKinds {
        fn definitions(&self) -> &[DirectoryKindDefinition] {
            &self.0
        }
    }

    fn service() -> UserService {
        let user = User::new(
            "alice",
            "hashed:old-password",
            UserRole::Member,
            DirectoryId::root(),
        )
        .unwrap();
        let directories = Arc::new(Directories::default());
        let users = Arc::new(Users {
            users: Mutex::new(vec![user]),
        });
        UserService::new(
            users.clone(),
            users,
            Arc::new(TestPasswordHasher),
            DirectoryService::new(
                directories.clone(),
                directories.clone(),
                directories,
                Arc::new(DirectoryKinds::default()),
            ),
        )
    }

    #[tokio::test]
    async fn updating_password_replaces_the_existing_credential() {
        let service = service();

        let updated = service
            .update_password(" alice ", "new-password")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.credential_hash(), "hashed:new-password");
        assert!(
            service
                .authenticate("alice", "old-password")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .authenticate("alice", "new-password")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn updating_password_enforces_the_four_character_minimum() {
        let service = service();

        assert!(matches!(
            service.update_password("alice", "abc").await,
            Err(CoreError::User(UserError::WeakPassword))
        ));
        assert!(
            service
                .update_password("alice", "abcd")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            service
                .update_password("unknown", "valid-password")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn creating_users_assigns_role_appropriate_default_workspaces() {
        let service = service();

        let member = service
            .create("bob", "member-password", UserRole::Member, None)
            .await
            .unwrap();
        let administrator = service
            .create("operator", "admin-password", UserRole::Administrator, None)
            .await
            .unwrap();

        assert_eq!(
            service
                .workspace_location(&member)
                .await
                .unwrap()
                .path()
                .path(),
            "users/bob"
        );
        assert!(administrator.workspace_directory_id().is_root());
    }

    #[tokio::test]
    async fn creating_user_preserves_an_explicit_workspace() {
        let service = service();
        let workspace = DirectoryPath::from_path("teams/bob").unwrap();

        let member = service
            .create(
                "bob",
                "member-password",
                UserRole::Member,
                Some(workspace.clone()),
            )
            .await
            .unwrap();

        assert_eq!(
            service.workspace_location(&member).await.unwrap().path(),
            &workspace
        );
    }
}
