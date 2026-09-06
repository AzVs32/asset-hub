//! Resource metadata, lifecycle, and durable relocation workflows.

use super::{ResourceService, UpdateResource, path_resolver};
use crate::CoreError;
use crate::domain::{DirectoryId, Resource, ResourceId};
use crate::port::{
    DirectoryLocation, ListResources, LocatedResource, ResourcePage, ResourceRelocation,
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
    /// The Blob is first moved to an internal staging key so a failed aggregate CAS can restore
    /// the visible file. The service loads the current location snapshot internally.
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
        let source_key = resource
            .content()
            .map(|_| located.storage_key())
            .transpose()?;
        let deletion_key = source_key
            .as_ref()
            .map(|_| path_resolver::deletion_key(resource.id()))
            .transpose()?;
        let keys = source_key
            .iter()
            .chain(deletion_key.iter())
            .cloned()
            .collect::<Vec<_>>();
        let _guards = self.storage_key_locks.lock_many(&keys).await;

        let moved = if let (Some(source), Some(staged)) = (&source_key, &deletion_key) {
            if self.objects.exists(source).await? {
                self.objects.move_if_absent(source, staged).await?;
                true
            } else {
                false
            }
        } else {
            false
        };

        if !self
            .store
            .delete_if_revision(&resource.id(), expected_revision)
            .await?
        {
            if moved && let (Some(source), Some(staged)) = (&source_key, &deletion_key) {
                self.objects.move_if_absent(staged, source).await?;
            }
            return Err(CoreError::revision_conflict(
                "resource",
                resource.id().to_string(),
            ));
        }

        if let Some(staged) = deletion_key {
            self.objects.delete(&staged).await?;
        }
        Ok(())
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
) -> crate::domain::ResourceBuilder {
    Resource::builder(name).with_directory_id(directory_id)
}
