//! Resource metadata, lifecycle, and durable relocation workflows.

use super::{ResourceService, UpdateResource, path_resolver};
use crate::CoreError;
use crate::{
    directory::{domain::DirectoryId, port::DirectoryLocation},
    resource::{
        domain::{Resource, ResourceDeletion, ResourceId},
        port::{ListResources, LocatedResource, ResourcePage, ResourceRelocation},
    },
};

impl ResourceService {
    pub async fn check_repository_health(&self) -> Result<(), CoreError> {
        self.store.health_check().await
    }

    pub async fn get(&self, id: &ResourceId) -> Result<Option<LocatedResource>, CoreError> {
        self.read_model.find_by_id(id).await
    }

    pub async fn list(&self, query: ListResources) -> Result<ResourcePage, CoreError> {
        self.read_model.list(&query).await
    }

    pub async fn locate_resource_directory(
        &self,
        resource: &Resource,
    ) -> Result<DirectoryLocation, CoreError> {
        self.directories
            .locate_by_id(&resource.directory_id())
            .await
    }

    /// Update a Resource by stable ID.
    ///
    /// The service loads the current location snapshot itself so callers do not need to depend on
    /// a read-model projection to perform a lifecycle operation.
    pub async fn update(
        &self,
        id: &ResourceId,
        command: UpdateResource,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(located) = self.get(id).await? else {
            return Ok(None);
        };
        self.update_located(located, command).await.map(Some)
    }

    async fn update_located(
        &self,
        located: LocatedResource,
        command: UpdateResource,
    ) -> Result<Resource, CoreError> {
        let (mut desired, source_directory) = located.into_parts();
        let expected_revision = desired.revision();
        let source_name = desired.name().to_string();
        if command.expected_revision != expected_revision {
            return Err(CoreError::revision_conflict(
                "resource",
                desired.id().to_string(),
            ));
        }

        if let Some(name) = command.name {
            desired.rename(name)?;
        }
        if let Some(directory_id) = command.directory_id {
            desired.move_to_directory(directory_id)?;
        }
        if desired.revision() == expected_revision {
            return Ok(desired);
        }

        let destination_directory = self
            .directories
            .locate_by_id(&desired.directory_id())
            .await?;
        if let Some(occupant) = self
            .read_model
            .find_by_directory_and_name(destination_directory.id(), desired.name())
            .await?
            && occupant.resource().id() != desired.id()
        {
            return Err(CoreError::conflict(format!(
                "resource path `{}` is already occupied",
                path_resolver::resource_key(destination_directory.path(), desired.name())?
            )));
        }

        let has_content = desired.content().is_some();
        let source_key = has_content
            .then(|| path_resolver::resource_key(source_directory.path(), &source_name))
            .transpose()?;
        let destination_key = has_content
            .then(|| path_resolver::resource_key(destination_directory.path(), desired.name()))
            .transpose()?;

        match (source_key, destination_key) {
            (Some(source_key), Some(destination_key)) if source_key != destination_key => {
                let _guards = self
                    .storage_key_locks
                    .lock_many(&[source_key.clone(), destination_key.clone()])
                    .await;
                // Fresh requests must not interpret an unrelated destination as a recovered move.
                // Recheck after acquiring the path locks: another writer may have changed the
                // resource while this request was waiting.
                let current = self.get(&desired.id()).await?;
                if !matches!(current.as_ref(), Some(current)
                    if current.resource().revision() == expected_revision
                        && current.storage_key()? == source_key)
                {
                    return Err(CoreError::revision_conflict(
                        "resource",
                        desired.id().to_string(),
                    ));
                }
                if !self.objects.exists(&source_key).await? {
                    return Err(CoreError::conflict(format!(
                        "physical source resource `{source_key}` is missing"
                    )));
                }
                if self.objects.exists(&destination_key).await? {
                    return Err(CoreError::conflict(format!(
                        "physical destination resource `{destination_key}` already exists"
                    )));
                }
                let relocation = ResourceRelocation::new(
                    desired.clone(),
                    expected_revision,
                    source_key,
                    destination_key,
                )?;
                self.relocations.save(&relocation).await?;
                self.recover_relocation_locked(&relocation).await?;
                Ok(desired)
            }
            _ => {
                if self
                    .store
                    .update_if_revision(&desired, expected_revision)
                    .await?
                {
                    Ok(desired)
                } else {
                    Err(CoreError::revision_conflict(
                        "resource",
                        desired.id().to_string(),
                    ))
                }
            }
        }
    }

    /// Permanently delete a Resource by stable ID and its physical content.
    ///
    /// A durable intent is saved before the visible Blob enters the internal deletion namespace.
    /// Recovery always moves forward after a database failure or interruption; a stale revision is
    /// resolved by preserving the newer Resource and never overwriting its visible Blob.
    pub async fn delete(&self, id: &ResourceId, expected_revision: u64) -> Result<bool, CoreError> {
        let Some(located) = self.get(id).await? else {
            return Ok(false);
        };
        self.delete_located(located, expected_revision).await?;
        Ok(true)
    }

    async fn delete_located(
        &self,
        located: LocatedResource,
        expected_revision: u64,
    ) -> Result<(), CoreError> {
        let resource = located.resource();
        if resource.revision() != expected_revision {
            return Err(CoreError::revision_conflict(
                "resource",
                resource.id().to_string(),
            ));
        }
        let Some(source_key) = resource
            .content()
            .map(|_| located.storage_key())
            .transpose()?
        else {
            if !self
                .store
                .delete_if_revision(&resource.id(), expected_revision)
                .await?
            {
                return Err(CoreError::revision_conflict(
                    "resource",
                    resource.id().to_string(),
                ));
            }
            return Ok(());
        };
        let deletion = ResourceDeletion::new(
            resource.id(),
            expected_revision,
            source_key,
            path_resolver::deletion_key(resource.id())?,
        )?;
        let _guards = self
            .storage_key_locks
            .lock_many(&[
                deletion.source_key().clone(),
                deletion.deletion_key().clone(),
            ])
            .await;
        self.deletions.save(&deletion).await?;
        self.recover_deletion_locked(&deletion).await
    }

    pub async fn recover_pending_relocations(&self) -> Result<usize, CoreError> {
        let relocations = self.relocations.load_all().await?;
        let count = relocations.len();
        for relocation in relocations {
            let _guards = self
                .storage_key_locks
                .lock_many(&[
                    relocation.source_key().clone(),
                    relocation.destination_key().clone(),
                ])
                .await;
            self.recover_relocation_locked(&relocation).await?;
        }
        Ok(count)
    }

    /// Complete or safely abandon permanent deletions that were interrupted between moving a Blob,
    /// committing the Resource CAS, and removing the internal staged file.
    pub async fn recover_pending_deletions(&self) -> Result<usize, CoreError> {
        let deletions = self.deletions.list_pending().await?;
        let count = deletions.len();
        for deletion in deletions {
            let _guards = self
                .storage_key_locks
                .lock_many(&[
                    deletion.source_key().clone(),
                    deletion.deletion_key().clone(),
                ])
                .await;
            self.recover_deletion_locked(&deletion).await?;
        }
        Ok(count)
    }

    async fn recover_deletion_locked(&self, deletion: &ResourceDeletion) -> Result<(), CoreError> {
        let Some(current) = self.store.load(&deletion.resource_id()).await? else {
            // The database CAS committed before cleanup. Never delete a newly-created visible file
            // at the old path; only the private staged object belongs to this intent.
            self.objects.delete(deletion.deletion_key()).await?;
            self.deletions.remove(&deletion.resource_id()).await?;
            return Ok(());
        };

        if current.revision() != deletion.expected_revision() {
            return self.resolve_stale_deletion_locked(deletion).await;
        }

        let source_exists = self.objects.exists(deletion.source_key()).await?;
        let staged_exists = self.objects.exists(deletion.deletion_key()).await?;
        match (source_exists, staged_exists) {
            (true, false) => {
                self.objects
                    .move_if_absent(deletion.source_key(), deletion.deletion_key())
                    .await?
            }
            (false, true) => {}
            (false, false) => tracing::warn!(
                resource_id = %deletion.resource_id(),
                source_key = %deletion.source_key(),
                "permanent deletion found an already-missing physical Blob; committing metadata deletion"
            ),
            (true, true) => {
                return Err(CoreError::conflict(format!(
                    "resource deletion `{}` has both visible and staged Blob objects",
                    deletion.resource_id()
                )));
            }
        }

        if self
            .store
            .delete_if_revision(&deletion.resource_id(), deletion.expected_revision())
            .await?
        {
            self.objects.delete(deletion.deletion_key()).await?;
            self.deletions.remove(&deletion.resource_id()).await?;
            return Ok(());
        }

        // A false CAS is either a committed delete observed through another connection or a newer
        // revision. Re-read before deciding whether this intent may be removed.
        if self.store.load(&deletion.resource_id()).await?.is_none() {
            self.objects.delete(deletion.deletion_key()).await?;
            self.deletions.remove(&deletion.resource_id()).await?;
            return Ok(());
        }
        self.resolve_stale_deletion_locked(deletion).await
    }

    async fn resolve_stale_deletion_locked(
        &self,
        deletion: &ResourceDeletion,
    ) -> Result<(), CoreError> {
        let current_key = self
            .get(&deletion.resource_id())
            .await?
            .map(|located| located.storage_key())
            .transpose()?;
        let staged_exists = self.objects.exists(deletion.deletion_key()).await?;

        // A newer Resource may have moved or renamed the same Blob after this deletion staged it.
        // Restore to its current key only when that key is vacant; `move_if_absent` preserves any
        // independently written replacement content.
        if staged_exists {
            if let Some(current_key) = current_key {
                if self.objects.exists(&current_key).await? {
                    self.objects.delete(deletion.deletion_key()).await?;
                } else {
                    self.objects
                        .move_if_absent(deletion.deletion_key(), &current_key)
                        .await?;
                }
            } else {
                self.objects.delete(deletion.deletion_key()).await?;
            }
        }
        self.deletions.remove(&deletion.resource_id()).await?;
        Err(CoreError::revision_conflict(
            "resource",
            deletion.resource_id().to_string(),
        ))
    }

    async fn recover_relocation_locked(
        &self,
        relocation: &ResourceRelocation,
    ) -> Result<(), CoreError> {
        let id = relocation.resource_id();
        let current = self
            .store
            .load(&id)
            .await?
            .ok_or_else(|| CoreError::not_found("resource relocation", id.to_string()))?;
        let source_exists = self.objects.exists(relocation.source_key()).await?;
        let destination_exists = self.objects.exists(relocation.destination_key()).await?;

        if current == *relocation.desired() {
            match (source_exists, destination_exists) {
                (true, false) => {
                    self.objects
                        .move_if_absent(relocation.source_key(), relocation.destination_key())
                        .await?;
                }
                (false, true) => {}
                _ => {
                    return Err(CoreError::conflict(format!(
                        "resource relocation `{id}` has ambiguous physical state"
                    )));
                }
            }
            self.relocations.complete(&id).await?;
            return Ok(());
        }

        if current.revision() != relocation.expected_revision() {
            if !source_exists && destination_exists {
                self.objects
                    .move_if_absent(relocation.destination_key(), relocation.source_key())
                    .await?;
            }
            self.relocations.complete(&id).await?;
            return Err(CoreError::revision_conflict("resource", id.to_string()));
        }

        match (source_exists, destination_exists) {
            (true, false) => {
                self.objects
                    .move_if_absent(relocation.source_key(), relocation.destination_key())
                    .await?;
            }
            (false, true) => {}
            _ => {
                return Err(CoreError::conflict(format!(
                    "resource relocation `{id}` has ambiguous physical state"
                )));
            }
        }

        if !self
            .store
            .update_if_revision(relocation.desired(), relocation.expected_revision())
            .await?
        {
            self.objects
                .move_if_absent(relocation.destination_key(), relocation.source_key())
                .await?;
            self.relocations.complete(&id).await?;
            return Err(CoreError::revision_conflict("resource", id.to_string()));
        }
        self.relocations.complete(&id).await
    }
}

pub(crate) fn build_resource(
    name: String,
    directory_id: DirectoryId,
) -> crate::resource::domain::ResourceBuilder {
    Resource::builder(name).with_directory_id(directory_id)
}
