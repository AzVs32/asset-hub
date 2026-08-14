use super::{DirectoryService, UpdateDirectory};
use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryKind, DirectoryPath},
    port::{DirectoryLocation, LocatedDirectory},
};

impl DirectoryService {
    /// Ensures every aggregate and physical directory in a path exists.
    pub async fn ensure_path(&self, path: &DirectoryPath) -> Result<DirectoryLocation, CoreError> {
        let _guard = self.mutation_lock.lock().await;
        if let Some(directory) = self.index.find_by_path(path).await? {
            self.storage.ensure_directory(path).await?;
            return Ok(directory.location().clone());
        }

        self.ensure_kind_registered(&DirectoryKind::default())?;
        self.storage.ensure_directory(path).await?;
        let mut parent = self
            .index
            .find_by_id(&DirectoryId::root())
            .await?
            .ok_or_else(|| CoreError::invariant("root directory is missing"))?;
        let mut current_path = DirectoryPath::root();
        for name in path.path().split('/').filter(|name| !name.is_empty()) {
            current_path = current_path.child(name)?;
            if let Some(existing) = self.index.find_by_path(&current_path).await? {
                parent = existing;
                continue;
            }
            let kind = self.kind_for_new_child(parent.directory().kind(), DirectoryKind::default());
            self.ensure_kind_registered(&kind)?;
            self.ensure_parent_kind_allowed(&kind, parent.directory().kind())?;
            let directory = Directory::new_with_kind(parent.id(), name, kind)?;
            self.repository.insert(&directory).await?;
            self.update_index(directory.clone()).await?;
            parent = LocatedDirectory::new(
                directory.clone(),
                DirectoryLocation::new(directory.id(), current_path.clone()),
            )?;
        }
        Ok(parent.location().clone())
    }

    pub async fn create(
        &self,
        parent: &DirectoryLocation,
        name: impl Into<String>,
    ) -> Result<DirectoryLocation, CoreError> {
        self.create_with_kind(parent, name, DirectoryKind::default())
            .await
            .map(|directory| directory.location().clone())
    }

    pub async fn create_with_kind(
        &self,
        parent: &DirectoryLocation,
        name: impl Into<String>,
        kind: DirectoryKind,
    ) -> Result<LocatedDirectory, CoreError> {
        self.create_with_kind_guarded(&parent.id(), name, kind, None, None)
            .await
    }

    pub(crate) async fn create_with_kind_in_scope(
        &self,
        parent: &DirectoryLocation,
        name: impl Into<String>,
        kind: DirectoryKind,
        scope_root: DirectoryId,
    ) -> Result<LocatedDirectory, CoreError> {
        self.create_with_kind_guarded(&parent.id(), name, kind, None, Some(scope_root))
            .await
    }

    pub(super) async fn create_with_kind_guarded(
        &self,
        parent_id: &DirectoryId,
        name: impl Into<String>,
        kind: DirectoryKind,
        expected_parent_revision: Option<u64>,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<LocatedDirectory, CoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.ensure_kind_registered(&kind)?;
        let parent = self
            .index
            .find_by_id(parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        if expected_parent_revision
            .is_some_and(|expected| expected != parent.directory().revision())
        {
            return Err(CoreError::conflict(format!(
                "directory `{parent_id}` changed while its action was executing"
            )));
        }
        if let Some(ancestor_id) = required_parent_ancestor
            && !self
                .index
                .is_descendant_or_self(&ancestor_id, parent_id)
                .await?
        {
            return Err(CoreError::forbidden(
                "create directory",
                parent.path().path(),
            ));
        }
        let kind = self.kind_for_new_child(parent.directory().kind(), kind);
        self.ensure_kind_registered(&kind)?;
        self.ensure_parent_kind_allowed(&kind, parent.directory().kind())?;
        let directory = Directory::new_with_kind(*parent_id, name, kind)?;
        let path = parent.path().child(directory.name())?;
        if self.index.find_by_path(&path).await?.is_some() {
            return Err(CoreError::conflict(
                "a directory with the same name already exists",
            ));
        }
        self.storage.ensure_directory(&path).await?;
        self.repository.insert(&directory).await?;
        self.update_index(directory.clone()).await?;
        LocatedDirectory::new(
            directory.clone(),
            DirectoryLocation::new(directory.id(), path),
        )
    }

    pub async fn update(
        &self,
        id: &DirectoryId,
        command: UpdateDirectory,
    ) -> Result<LocatedDirectory, CoreError> {
        self.update_expected(id, command, None).await
    }

    pub(super) async fn update_expected(
        &self,
        id: &DirectoryId,
        command: UpdateDirectory,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<LocatedDirectory, CoreError> {
        let _guard = self.mutation_lock.lock().await;
        let located = self
            .index
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))?;
        let (mut directory, from) = located.into_parts();
        let expected_revision = directory.revision();
        if command.expected_revision != expected_revision {
            return Err(CoreError::revision_conflict("directory", id.to_string()));
        }

        if directory.id().is_root()
            && (command.name.is_some() || command.parent_id.is_some() || command.kind.is_some())
        {
            return Err(CoreError::conflict("root directory cannot be updated"));
        }
        if let Some(ancestor_id) = required_parent_ancestor
            && !self.index.is_descendant_or_self(&ancestor_id, id).await?
        {
            return Err(CoreError::forbidden("update directory", from.path().path()));
        }
        let destination_kind = command
            .kind
            .clone()
            .unwrap_or_else(|| directory.kind().clone());
        self.ensure_kind_registered(&destination_kind)?;
        let parent_id = command
            .parent_id
            .or(directory.parent_id())
            .ok_or_else(|| CoreError::invariant("non-root directory is missing its parent"))?;
        if self
            .index
            .is_descendant_or_self(&directory.id(), &parent_id)
            .await?
        {
            return Err(CoreError::conflict(
                "moving the directory would create a cycle",
            ));
        }
        let parent = self
            .index
            .find_by_id(&parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        if let Some(ancestor_id) = required_parent_ancestor
            && !self
                .index
                .is_descendant_or_self(&ancestor_id, &parent_id)
                .await?
        {
            return Err(CoreError::forbidden(
                "update directory",
                parent.path().path(),
            ));
        }
        self.ensure_parent_kind_allowed(&destination_kind, parent.directory().kind())?;
        let default_child_kind = command.kind.as_ref().and_then(|_| {
            self.kind_registry
                .get(&destination_kind)
                .and_then(|definition| definition.default_child_kind())
                .cloned()
        });
        let mut child_kind_changes = Vec::new();
        for child in self.index.list_children(&directory.id()).await? {
            let desired_kind = if child.directory().kind() == &DirectoryKind::default() {
                default_child_kind
                    .clone()
                    .unwrap_or_else(|| child.directory().kind().clone())
            } else {
                child.directory().kind().clone()
            };
            self.ensure_kind_registered(&desired_kind)?;
            self.ensure_parent_kind_allowed(&desired_kind, &destination_kind)?;
            if desired_kind != *child.directory().kind() {
                let original = child.directory().clone();
                let mut updated = original.clone();
                updated.change_kind(desired_kind);
                child_kind_changes.push((original, updated));
            }
        }
        if let Some(kind) = command.kind {
            directory.change_kind(kind);
        }
        if let Some(name) = command.name {
            directory.rename(name)?;
        }
        directory.move_to(parent_id)?;
        let destination = parent.path().child(directory.name())?;
        if let Some(existing) = self.index.find_by_path(&destination).await?
            && existing.id() != directory.id()
        {
            return Err(CoreError::conflict(
                "a directory with the same name already exists",
            ));
        }
        self.persist_child_kind_changes(&child_kind_changes).await?;
        if directory.revision() == expected_revision {
            return LocatedDirectory::new(directory, from);
        }

        let moved = destination != *from.path();
        if moved && let Err(error) = self.storage.move_directory(from.path(), &destination).await {
            self.rollback_child_kind_changes(&child_kind_changes)
                .await?;
            return Err(error);
        }
        let saved = self
            .repository
            .save_if_unchanged(&directory, expected_revision)
            .await;
        let error = match saved {
            Ok(true) => {
                self.update_index(directory.clone()).await?;
                return LocatedDirectory::new(
                    directory.clone(),
                    DirectoryLocation::new(directory.id(), destination),
                );
            }
            Ok(false) => CoreError::conflict(format!(
                "directory `{}` changed while it was being updated",
                directory.id()
            )),
            Err(error) => error,
        };
        let child_rollback = self.rollback_child_kind_changes(&child_kind_changes).await;
        let storage_rollback = if moved {
            self.storage.move_directory(&destination, from.path()).await
        } else {
            Ok(())
        };
        if let Err(rollback) = storage_rollback {
            return Err(CoreError::storage("directory.update.rollback", rollback));
        }
        child_rollback?;
        Err(error)
    }

    async fn persist_child_kind_changes(
        &self,
        changes: &[(Directory, Directory)],
    ) -> Result<(), CoreError> {
        let mut persisted = 0;
        for (original, updated) in changes {
            match self
                .repository
                .save_if_unchanged(updated, original.revision())
                .await
            {
                Ok(true) => persisted += 1,
                Ok(false) => {
                    self.rollback_child_kind_changes(&changes[..persisted])
                        .await?;
                    return Err(CoreError::conflict(format!(
                        "directory `{}` changed while its parent kind was being updated",
                        original.id()
                    )));
                }
                Err(error) => {
                    self.rollback_child_kind_changes(&changes[..persisted])
                        .await?;
                    return Err(error);
                }
            }
        }
        if !changes.is_empty()
            && let Err(error) = self.reload_index().await
        {
            self.rollback_child_kind_changes(changes).await?;
            return Err(error);
        }
        Ok(())
    }

    async fn rollback_child_kind_changes(
        &self,
        changes: &[(Directory, Directory)],
    ) -> Result<(), CoreError> {
        for (original, updated) in changes.iter().rev() {
            let mut restored = updated.clone();
            restored.change_kind(original.kind().clone());
            if !self
                .repository
                .save_if_unchanged(&restored, updated.revision())
                .await?
            {
                let _ = self.reload_index().await;
                return Err(CoreError::conflict(format!(
                    "directory `{}` changed while its automatic kind update was rolling back",
                    original.id()
                )));
            }
        }
        if !changes.is_empty() {
            self.reload_index().await?;
        }
        Ok(())
    }

    pub async fn rename(
        &self,
        id: &DirectoryId,
        name: impl Into<String>,
    ) -> Result<DirectoryLocation, CoreError> {
        let current = self.find_by_id(id).await?;
        self.update(
            id,
            UpdateDirectory::new(current.directory().revision()).with_name(name),
        )
        .await
        .map(|directory| directory.location().clone())
    }

    pub async fn move_to(
        &self,
        id: &DirectoryId,
        parent_id: &DirectoryId,
    ) -> Result<DirectoryLocation, CoreError> {
        let current = self.find_by_id(id).await?;
        self.update(
            id,
            UpdateDirectory::new(current.directory().revision()).with_parent_id(*parent_id),
        )
        .await
        .map(|directory| directory.location().clone())
    }

    pub async fn change_kind(
        &self,
        id: &DirectoryId,
        kind: DirectoryKind,
    ) -> Result<LocatedDirectory, CoreError> {
        let current = self.find_by_id(id).await?;
        self.update(
            id,
            UpdateDirectory::new(current.directory().revision()).with_kind(kind),
        )
        .await
    }

    pub async fn remove_if_empty(
        &self,
        directory: &DirectoryLocation,
        caller_revision: Option<u64>,
    ) -> Result<bool, CoreError> {
        if directory.id().is_root() {
            return Ok(false);
        }
        let _guard = self.mutation_lock.lock().await;
        let current = self
            .index
            .find_by_id(&directory.id())
            .await?
            .ok_or_else(|| CoreError::not_found("directory", directory.id().to_string()))?;
        let revision = current.directory().revision();
        if caller_revision.is_some_and(|expected| expected != revision) {
            return Err(CoreError::revision_conflict(
                "directory",
                directory.id().to_string(),
            ));
        }
        if self
            .repository
            .remove_if_empty(&directory.id(), revision)
            .await?
        {
            self.index.remove(&directory.id()).await?;
            return Ok(true);
        }
        Ok(false)
    }
}
