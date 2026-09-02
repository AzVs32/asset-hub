use super::{DirectoryService, UpdateDirectory};
use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryKind},
    port::{DirectoryLocation, DirectoryRelocation, DirectoryRevisionUpdate, LocatedDirectory},
};

impl DirectoryService {
    pub async fn create(
        &self,
        parent_id: &DirectoryId,
        name: impl Into<String>,
    ) -> Result<DirectoryLocation, CoreError> {
        self.create_with_kind(parent_id, name, DirectoryKind::default())
            .await
            .map(|directory| directory.location().clone())
    }

    pub async fn create_with_kind(
        &self,
        parent_id: &DirectoryId,
        name: impl Into<String>,
        kind: DirectoryKind,
    ) -> Result<LocatedDirectory, CoreError> {
        self.create_with_kind_guarded(parent_id, name, kind, None, None)
            .await
    }

    pub(crate) async fn create_with_kind_in_scope(
        &self,
        parent_id: &DirectoryId,
        name: impl Into<String>,
        kind: DirectoryKind,
        scope_root: DirectoryId,
    ) -> Result<LocatedDirectory, CoreError> {
        self.create_with_kind_guarded(parent_id, name, kind, None, Some(scope_root))
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
        let _guard = self.kernel.mutation_lock.lock().await;
        self.ensure_kind_registered(&kind)?;
        let parent = self
            .kernel
            .query
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
                .kernel
                .query
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
        if self.kernel.query.find_by_path(&path).await?.is_some() {
            return Err(CoreError::conflict(
                "a directory with the same name already exists",
            ));
        }

        let physically_existed = self.kernel.storage.directory_exists(&path).await?;
        self.kernel.storage.ensure_directory(&path).await?;
        if let Err(error) = self.kernel.store.insert(&directory).await {
            if !physically_existed {
                let _ = self.kernel.storage.delete_empty_directory(&path).await;
            }
            return Err(error);
        }
        self.refresh_index(&directory.id()).await?;
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
        let _guard = self.kernel.mutation_lock.lock().await;
        let located = self
            .kernel
            .query
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
            && !self
                .kernel
                .query
                .is_descendant_or_self(&ancestor_id, id)
                .await?
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
            .kernel
            .query
            .is_descendant_or_self(&directory.id(), &parent_id)
            .await?
        {
            return Err(CoreError::conflict(
                "moving the directory would create a cycle",
            ));
        }
        let parent = self
            .kernel
            .query
            .find_by_id(&parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        if let Some(ancestor_id) = required_parent_ancestor
            && !self
                .kernel
                .query
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
            self.kernel
                .kind_registry
                .get(&destination_kind)
                .and_then(|definition| definition.default_child_kind())
                .cloned()
        });
        let mut child_updates = Vec::new();
        for child in self.kernel.query.list_children(&directory.id()).await? {
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
                let expected = child.directory().revision();
                let mut updated = child.into_directory();
                updated.change_kind(desired_kind);
                child_updates.push(DirectoryRevisionUpdate::new(updated, expected)?);
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
        if let Some(existing) = self.kernel.query.find_by_path(&destination).await?
            && existing.id() != directory.id()
        {
            return Err(CoreError::conflict(
                "a directory with the same name already exists",
            ));
        }
        if directory.revision() == expected_revision && child_updates.is_empty() {
            return LocatedDirectory::new(directory, from);
        }

        let mut updates = Vec::with_capacity(1 + child_updates.len());
        if directory.revision() > expected_revision {
            updates.push(DirectoryRevisionUpdate::new(
                directory.clone(),
                expected_revision,
            )?);
        }
        updates.extend(child_updates);
        if destination == *from.path() {
            if !self
                .kernel
                .store
                .update_batch_if_unchanged(&updates)
                .await?
            {
                return Err(CoreError::conflict(format!(
                    "directory `{id}` changed while it was being updated"
                )));
            }
            self.kernel.index_service.rebuild().await?;
        } else {
            if !self.kernel.storage.directory_exists(from.path()).await? {
                return Err(CoreError::conflict(format!(
                    "physical source directory `{}` is missing",
                    from.path()
                )));
            }
            if self.kernel.storage.directory_exists(&destination).await? {
                return Err(CoreError::conflict(format!(
                    "physical destination directory `{destination}` already exists"
                )));
            }
            let relocation =
                DirectoryRelocation::new(*id, from.path().clone(), destination.clone(), updates)?;
            self.kernel.relocations.begin(&relocation).await?;
            self.finish_relocation(&relocation, false).await?;
        }

        LocatedDirectory::new(
            directory.clone(),
            DirectoryLocation::new(directory.id(), destination),
        )
    }

    /// Complete every durable Directory relocation left by an interrupted process.
    pub async fn recover_pending_relocations(&self) -> Result<u64, CoreError> {
        let _guard = self.kernel.mutation_lock.lock().await;
        let pending = self.kernel.relocations.load_pending().await?;
        for relocation in &pending {
            self.finish_relocation(relocation, true).await?;
        }
        Ok(pending.len() as u64)
    }

    async fn finish_relocation(
        &self,
        relocation: &DirectoryRelocation,
        recovering: bool,
    ) -> Result<(), CoreError> {
        let mut all_expected = true;
        let mut all_applied = true;
        for update in relocation.updates() {
            let current = self
                .kernel
                .store
                .load(&update.directory().id())
                .await?
                .ok_or_else(|| {
                    CoreError::invariant(format!(
                        "pending relocation references missing directory `{}`",
                        update.directory().id()
                    ))
                })?;
            all_expected &= current.revision() == update.expected_revision();
            all_applied &= current == *update.directory();
        }
        if !all_expected && !all_applied {
            return Err(CoreError::conflict(format!(
                "pending relocation for directory `{}` conflicts with persisted revisions",
                relocation.directory_id()
            )));
        }

        let source_exists = self
            .kernel
            .storage
            .directory_exists(relocation.source())
            .await?;
        let destination_exists = self
            .kernel
            .storage
            .directory_exists(relocation.destination())
            .await?;
        match (source_exists, destination_exists) {
            (true, false) => {
                if let Err(error) = self
                    .kernel
                    .storage
                    .move_directory(relocation.source(), relocation.destination())
                    .await
                {
                    if !recovering {
                        self.kernel
                            .relocations
                            .complete(&relocation.directory_id())
                            .await?;
                    }
                    return Err(error);
                }
            }
            (false, true) => {}
            (true, true) => {
                if !recovering {
                    self.kernel
                        .relocations
                        .complete(&relocation.directory_id())
                        .await?;
                }
                return Err(CoreError::conflict(format!(
                    "both source and destination exist for pending directory relocation `{}`",
                    relocation.directory_id()
                )));
            }
            (false, false) => {
                if !recovering {
                    self.kernel
                        .relocations
                        .complete(&relocation.directory_id())
                        .await?;
                }
                return Err(CoreError::invariant(format!(
                    "neither source nor destination exists for pending directory relocation `{}`",
                    relocation.directory_id()
                )));
            }
        }

        if all_expected {
            match self
                .kernel
                .store
                .update_batch_if_unchanged(relocation.updates())
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    self.rollback_relocation(relocation).await?;
                    return Err(CoreError::conflict(format!(
                        "directory `{}` changed while its relocation was committed",
                        relocation.directory_id()
                    )));
                }
                Err(error) => return Err(error),
            }
        }
        self.kernel.index_service.rebuild().await?;
        self.kernel
            .relocations
            .complete(&relocation.directory_id())
            .await
    }

    async fn rollback_relocation(&self, relocation: &DirectoryRelocation) -> Result<(), CoreError> {
        self.kernel
            .storage
            .move_directory(relocation.destination(), relocation.source())
            .await
            .map_err(|error| CoreError::storage("directory.relocation.rollback", error))?;
        self.kernel
            .relocations
            .complete(&relocation.directory_id())
            .await
    }

    pub async fn delete_if_empty(
        &self,
        id: &DirectoryId,
        caller_revision: Option<u64>,
    ) -> Result<bool, CoreError> {
        if id.is_root() {
            return Ok(false);
        }
        let _guard = self.kernel.mutation_lock.lock().await;
        let current = self
            .kernel
            .query
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))?;
        let revision = current.directory().revision();
        if caller_revision.is_some_and(|expected| expected != revision) {
            return Err(CoreError::revision_conflict("directory", id.to_string()));
        }
        if !self.kernel.store.is_empty(id).await? {
            return Ok(false);
        }

        self.kernel
            .storage
            .delete_empty_directory(current.path())
            .await?;
        match self.kernel.store.delete_if_empty(id, revision).await {
            Ok(true) => {
                self.kernel.index_service.remove(id).await?;
                Ok(true)
            }
            Ok(false) => {
                self.kernel.storage.ensure_directory(current.path()).await?;
                Ok(false)
            }
            Err(error) => {
                self.kernel.storage.ensure_directory(current.path()).await?;
                Err(error)
            }
        }
    }
}
