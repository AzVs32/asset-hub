use crate::{
    CoreError,
    domain::{AccessContext, DirectoryPermission, ResourceDirectory},
    port::UserRepository,
};
use std::sync::Arc;

/// 当前访问主体看到的目录根。外部路径相对于该根，内部服务始终使用真实目录。
#[derive(Debug, Clone)]
pub struct WorkspaceScope {
    root: ResourceDirectory,
}

impl WorkspaceScope {
    pub fn resolve(&self, directory: &ResourceDirectory) -> Result<ResourceDirectory, CoreError> {
        if self.root.is_root() {
            return Ok(directory.clone());
        }
        if directory.is_root() {
            return Ok(self.root.clone());
        }
        ResourceDirectory::from_path(format!("{}/{}", self.root.path(), directory.path()))
            .map_err(Into::into)
    }

    pub fn project(&self, directory: &ResourceDirectory) -> Result<ResourceDirectory, CoreError> {
        if self.root.is_root() {
            return Ok(directory.clone());
        }
        if directory == &self.root {
            return Ok(ResourceDirectory::root());
        }
        let Some(relative) = directory
            .path()
            .strip_prefix(self.root.path())
            .and_then(|suffix| suffix.strip_prefix('/'))
        else {
            return Err(CoreError::forbidden("read", directory.path()));
        };
        ResourceDirectory::from_path(relative).map_err(Into::into)
    }

    fn contains(&self, directory: &ResourceDirectory) -> bool {
        self.root.contains(directory)
    }
}

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
        let scope = self.workspace_scope(context).await?;
        if scope.contains(directory) {
            return Ok(());
        }
        Err(CoreError::forbidden(
            permission_action(permission),
            directory.path(),
        ))
    }

    pub async fn workspace_scope(
        &self,
        context: &AccessContext,
    ) -> Result<WorkspaceScope, CoreError> {
        if context.is_administrator() {
            return Ok(WorkspaceScope {
                root: ResourceDirectory::root(),
            });
        }
        let user = self
            .users
            .find_by_id(&context.user_id())
            .await?
            .ok_or_else(|| CoreError::not_found("user", context.user_id().to_string()))?;
        Ok(WorkspaceScope {
            root: user.workspace_directory().clone(),
        })
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
