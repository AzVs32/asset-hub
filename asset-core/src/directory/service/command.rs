use super::{DirectoryService, UpdateDirectory};
use crate::{
    CoreError,
    directory::{
        domain::{Directory, DirectoryId},
        port::{DirectoryRelocation, DirectoryRevisionUpdate},
        query::LocatedDirectory,
    },
};

impl DirectoryService {
    /// Create a direct child below a global parent ID and return its aggregate with the resolved
    /// global location.
    pub async fn create(
        &self,
        parent_id: &DirectoryId,
        name: impl Into<String>,
    ) -> Result<LocatedDirectory, CoreError> {
        let _guard = self.kernel.mutation_lock.lock().await;
        let parent = self
            .kernel
            .read_model
            .find_by_id(parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        let directory = Directory::new(*parent_id, name)?;
        let path = parent.path().child(directory.name())?;
        if self.kernel.read_model.find_by_path(&path).await?.is_some() {
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
        Ok(LocatedDirectory::new(directory, path))
    }

    pub async fn update(
        &self,
        id: &DirectoryId,
        command: UpdateDirectory,
    ) -> Result<LocatedDirectory, CoreError> {
        let _guard = self.kernel.mutation_lock.lock().await;
        let located = self
            .kernel
            .read_model
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))?;
        let (mut directory, from) = located.into_parts();
        let expected_revision = directory.revision();
        if command.expected_revision != expected_revision {
            return Err(CoreError::revision_conflict("directory", id.to_string()));
        }

        if directory.is_root() {
            if command.name.is_some() || command.parent_id.is_some() {
                return Err(CoreError::conflict("root directory cannot be updated"));
            }
            return Ok(LocatedDirectory::new(directory, from));
        }
        let parent_id = command
            .parent_id
            .or(directory.parent_id())
            .ok_or_else(|| CoreError::invariant("non-root directory is missing its parent"))?;
        if self
            .kernel
            .read_model
            .is_descendant_or_self(&directory.id(), &parent_id)
            .await?
        {
            return Err(CoreError::conflict(
                "moving the directory would create a cycle",
            ));
        }
        let parent = self
            .kernel
            .read_model
            .find_by_id(&parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        if let Some(name) = command.name {
            directory.rename(name)?;
        }
        directory.move_to(parent_id)?;
        let destination = parent.path().child(directory.name())?;
        if let Some(existing) = self.kernel.read_model.find_by_path(&destination).await?
            && existing.id() != directory.id()
        {
            return Err(CoreError::conflict(
                "a directory with the same name already exists",
            ));
        }
        if directory.revision() == expected_revision {
            return Ok(LocatedDirectory::new(directory, from));
        }

        let mut updates = Vec::with_capacity(1);
        if directory.revision() > expected_revision {
            updates.push(DirectoryRevisionUpdate::new(
                directory.clone(),
                expected_revision,
            )?);
        }
        if destination == from {
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
            if !self.kernel.storage.directory_exists(&from).await? {
                return Err(CoreError::conflict(format!(
                    "physical source directory `{}` is missing",
                    from
                )));
            }
            if self.kernel.storage.directory_exists(&destination).await? {
                return Err(CoreError::conflict(format!(
                    "physical destination directory `{destination}` already exists"
                )));
            }
            let relocation = DirectoryRelocation::new(*id, from, destination.clone(), updates)?;
            self.kernel.relocations.begin(&relocation).await?;
            self.finish_relocation(&relocation, false).await?;
        }

        Ok(LocatedDirectory::new(directory, destination))
    }

    pub(super) async fn recover_pending_relocations(&self) -> Result<u64, CoreError> {
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

    /// Delete an empty non-root directory with an explicit revision precondition.
    pub async fn delete(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        self.delete_if_empty(id, Some(expected_revision)).await
    }

    /// Trusted storage maintenance may remove a directory only after its physical absence has
    /// been reconciled. Ordinary operations must use [`Self::delete`] instead.
    pub(in crate::directory) async fn delete_if_empty_for_maintenance(
        &self,
        id: &DirectoryId,
    ) -> Result<bool, CoreError> {
        self.delete_if_empty(id, None).await
    }

    async fn delete_if_empty(
        &self,
        id: &DirectoryId,
        caller_revision: Option<u64>,
    ) -> Result<bool, CoreError> {
        let _guard = self.kernel.mutation_lock.lock().await;
        let current = self
            .kernel
            .read_model
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))?;
        let revision = current.directory().revision();
        if current.directory().is_root() {
            return Ok(false);
        }
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
