use super::action::resolved_content_delivery;
use super::content::hex_sha256;
use super::*;
use crate::domain::{
    AccessContext, Checksum, ChecksumKind, ContentVerificationStatus, Directory,
    DirectoryActionAccess, DirectoryActionDefinition, DirectoryId, DirectoryPath,
    ResourceActionAccess, ResourceActionDefinition, ResourceActionPolicy,
    ResourceContentEditPolicy, ResourceContentMatcher, ResourceContentReplacement,
    ResourceContentReplacementId, ResourceId, UploadId, UploadSession, UploadStatus, User, UserId,
    UserRole,
};
use crate::port::{
    BlobByteStream, DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRegistry,
    DirectoryActionRequest, DirectoryIndex, DirectoryKindDefinition, DirectoryKindRegistry,
    DirectoryLocation, DirectoryQuery, DirectoryStore, ListResources, LocatedDirectory,
    LocatedResource, ResourceActionOutput, ResourceActionRequest,
    ResourceContentReplacementRepository, ResourceKindDefinition, ResourceKindRegistry,
    ResourcePage, ScannedStorageEntry, StagedBlob, StoragePrefix, UploadSessionRepository,
    UserRepository,
};
use asset_plugin_api::protocol::directory::{
    DirectoryActionEffect, DirectoryPluginActionOutput, UpdateDirectoryEffect,
};
use asset_plugin_api::protocol::{
    PluginActionEffect, PluginActionOutput, PluginReplacementEncoding, PluginView,
    ReplaceContentEffect, TextView,
};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use tokio::sync::oneshot;

#[derive(Default)]
struct InMemoryUploadSessionRepository {
    sessions: Mutex<HashMap<UploadId, UploadSession>>,
}

#[derive(Default)]
struct InMemoryContentReplacementRepository {
    replacements: Mutex<HashMap<ResourceContentReplacementId, ResourceContentReplacement>>,
}

#[async_trait::async_trait]
impl ResourceContentReplacementRepository for InMemoryContentReplacementRepository {
    async fn save(&self, replacement: &ResourceContentReplacement) -> Result<(), CoreError> {
        let mut replacements = self.replacements.lock().unwrap();
        if replacements
            .values()
            .any(|pending| pending.resource_id() == replacement.resource_id())
        {
            return Err(CoreError::conflict(format!(
                "resource `{}` already has a pending content replacement",
                replacement.resource_id()
            )));
        }
        replacements.insert(replacement.id(), replacement.clone());
        Ok(())
    }

    async fn list_pending(&self) -> Result<Vec<ResourceContentReplacement>, CoreError> {
        let mut replacements = self
            .replacements
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        replacements.sort_by_key(|replacement| replacement.id().to_string());
        Ok(replacements)
    }

    async fn remove(&self, id: &ResourceContentReplacementId) -> Result<(), CoreError> {
        self.replacements.lock().unwrap().remove(id);
        Ok(())
    }
}

#[async_trait::async_trait]
impl UploadSessionRepository for InMemoryUploadSessionRepository {
    async fn save(&self, session: &UploadSession) -> Result<(), CoreError> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id(), session.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &UploadId) -> Result<Option<UploadSession>, CoreError> {
        Ok(self.sessions.lock().unwrap().get(id).cloned())
    }

    async fn update_offset(
        &self,
        id: &UploadId,
        expected_offset: u64,
        offset: u64,
    ) -> Result<bool, CoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(id) else {
            return Ok(false);
        };
        if session.offset() != expected_offset {
            return Ok(false);
        }
        session.set_offset(offset);
        Ok(true)
    }

    async fn mark_finalizing(&self, id: &UploadId) -> Result<bool, CoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(id) else {
            return Ok(false);
        };
        if !matches!(
            session.status(),
            UploadStatus::Uploading | UploadStatus::Failed
        ) {
            return Ok(false);
        }
        session.mark_finalizing();
        Ok(true)
    }

    async fn save_actual_checksum(
        &self,
        id: &UploadId,
        checksum: &Checksum,
    ) -> Result<(), CoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| CoreError::not_found("upload", id.to_string()))?;
        session.set_actual_checksum(checksum.clone());
        Ok(())
    }

    async fn mark_completed(&self, id: &UploadId) -> Result<(), CoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| CoreError::not_found("upload", id.to_string()))?;
        session.mark_completed();
        Ok(())
    }

    async fn mark_failed(&self, id: &UploadId, failure: &str) -> Result<(), CoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| CoreError::not_found("upload", id.to_string()))?;
        session.mark_failed(failure);
        Ok(())
    }

    async fn list_finalizing(&self) -> Result<Vec<UploadId>, CoreError> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.status() == UploadStatus::Finalizing)
            .map(UploadSession::id)
            .collect())
    }

    async fn remove(&self, id: &UploadId) -> Result<(), CoreError> {
        self.sessions.lock().unwrap().remove(id);
        Ok(())
    }
}

struct InMemoryResourceRepository {
    resources: Mutex<HashMap<ResourceId, Resource>>,
    directories: Mutex<HashMap<DirectoryId, (Directory, DirectoryPath)>>,
    fail_next_save: Mutex<bool>,
    fail_next_conditional_save: Mutex<bool>,
    next_save_started: Mutex<Option<oneshot::Sender<()>>>,
    next_save_release: Mutex<Option<oneshot::Receiver<()>>>,
}

impl Default for InMemoryResourceRepository {
    fn default() -> Self {
        let root = Directory::root();
        Self {
            resources: Mutex::new(HashMap::new()),
            directories: Mutex::new(HashMap::from([(root.id(), (root, DirectoryPath::root()))])),
            fail_next_save: Mutex::new(false),
            fail_next_conditional_save: Mutex::new(false),
            next_save_started: Mutex::new(None),
            next_save_release: Mutex::new(None),
        }
    }
}

impl InMemoryResourceRepository {
    fn fail_next_save(&self) {
        *self.fail_next_save.lock().unwrap() = true;
    }

    fn fail_next_conditional_save(&self) {
        *self.fail_next_conditional_save.lock().unwrap() = true;
    }

    fn find_sync(&self, id: &ResourceId) -> Option<Resource> {
        self.resources.lock().unwrap().get(id).cloned()
    }

    fn locate_sync(&self, resource: Resource) -> LocatedResource {
        let path = self
            .directories
            .lock()
            .unwrap()
            .get(&resource.directory_id())
            .unwrap()
            .1
            .clone();
        LocatedResource::new(
            resource.clone(),
            DirectoryLocation::new(resource.directory_id(), path),
        )
        .unwrap()
    }

    fn is_empty(&self) -> bool {
        self.resources.lock().unwrap().is_empty()
    }

    fn len(&self) -> usize {
        self.resources.lock().unwrap().len()
    }

    fn pause_next_save(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (started_sender, started_receiver) = oneshot::channel();
        let (release_sender, release_receiver) = oneshot::channel();
        *self.next_save_started.lock().unwrap() = Some(started_sender);
        *self.next_save_release.lock().unwrap() = Some(release_receiver);
        (started_receiver, release_sender)
    }
}

#[async_trait::async_trait]
impl ResourceRepository for InMemoryResourceRepository {
    async fn health_check(&self) -> Result<(), CoreError> {
        Ok(())
    }

    async fn save(&self, resource: &Resource) -> Result<(), CoreError> {
        if std::mem::take(&mut *self.fail_next_save.lock().unwrap()) {
            return Err(CoreError::repository("save", TestError("save failed")));
        }
        let started = self.next_save_started.lock().unwrap().take();
        let release = self.next_save_release.lock().unwrap().take();
        if let Some(started) = started {
            let _ = started.send(());
        }
        if let Some(release) = release {
            let _ = release.await;
        }

        self.resources
            .lock()
            .unwrap()
            .insert(resource.id(), resource.clone());

        Ok(())
    }

    async fn save_if_unchanged(
        &self,
        resource: &Resource,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        if std::mem::take(&mut *self.fail_next_conditional_save.lock().unwrap()) {
            return Err(CoreError::repository(
                "save_if_unchanged",
                TestError("conditional save failed"),
            ));
        }
        let started = self.next_save_started.lock().unwrap().take();
        let release = self.next_save_release.lock().unwrap().take();
        if let Some(started) = started {
            let _ = started.send(());
        }
        if let Some(release) = release {
            let _ = release.await;
        }
        {
            let resources = self.resources.lock().unwrap();
            let Some(current) = resources.get(&resource.id()) else {
                return Ok(false);
            };
            if current.revision() != expected_revision {
                return Ok(false);
            }
        }
        let mut resources = self.resources.lock().unwrap();
        resources.insert(resource.id(), resource.clone());
        Ok(true)
    }

    async fn remove_if_unchanged(
        &self,
        id: &ResourceId,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let mut resources = self.resources.lock().unwrap();
        let Some(current) = resources.get(id) else {
            return Ok(false);
        };
        if current.revision() != expected_revision {
            return Ok(false);
        }
        resources.remove(id);
        Ok(true)
    }

    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        Ok(self.find_sync(id))
    }

    async fn remove(&self, id: &ResourceId) -> Result<(), CoreError> {
        self.resources.lock().unwrap().remove(id);
        Ok(())
    }
}

#[async_trait::async_trait]
impl ResourceQuery for InMemoryResourceRepository {
    async fn find_located_by_id(
        &self,
        id: &ResourceId,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let Some(resource) = self.find_sync(id) else {
            return Ok(None);
        };
        let path = self
            .directories
            .lock()
            .unwrap()
            .get(&resource.directory_id())
            .map(|(_, path)| path.clone())
            .ok_or_else(|| {
                CoreError::not_found("directory", resource.directory_id().to_string())
            })?;
        LocatedResource::new(
            resource.clone(),
            DirectoryLocation::new(resource.directory_id(), path),
        )
        .map(Some)
    }

    async fn find_by_path(
        &self,
        directory: &DirectoryPath,
        name: &str,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let directory_id = self
            .directories
            .lock()
            .unwrap()
            .iter()
            .find(|(_, (_, candidate))| candidate == directory)
            .map(|(id, _)| *id);
        self.resources
            .lock()
            .unwrap()
            .values()
            .find(|resource| {
                !resource.is_deleted()
                    && Some(resource.directory_id()) == directory_id
                    && resource.name() == name
            })
            .cloned()
            .map(|resource| {
                LocatedResource::new(
                    resource,
                    DirectoryLocation::new(directory_id.unwrap(), directory.clone()),
                )
            })
            .transpose()
    }

    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError> {
        let mut resources = self
            .resources
            .lock()
            .unwrap()
            .values()
            .filter(|resource| query.include_deleted() || !resource.is_deleted())
            .filter(|resource| query.kinds().is_empty() || query.kinds().contains(resource.kind()))
            .filter(|resource| {
                query
                    .tag()
                    .is_none_or(|tag| resource.tags().iter().any(|value| value.as_str() == tag))
            })
            .filter(|resource| query.q().is_none_or(|q| resource.name().contains(q)))
            .filter(|resource| {
                query
                    .directory_id()
                    .is_none_or(|directory_id| resource.directory_id() == *directory_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        resources.sort_by_key(|resource| std::cmp::Reverse(resource.updated_at()));

        let total = resources.len() as u64;
        let items = resources
            .into_iter()
            .skip(query.offset() as usize)
            .take(query.limit() as usize)
            .map(|resource| {
                let path = self
                    .directories
                    .lock()
                    .unwrap()
                    .get(&resource.directory_id())
                    .map(|(_, path)| path.clone())
                    .ok_or_else(|| {
                        CoreError::not_found("directory", resource.directory_id().to_string())
                    })?;
                LocatedResource::new(
                    resource.clone(),
                    DirectoryLocation::new(resource.directory_id(), path),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ResourcePage {
            items,
            total,
            limit: query.limit(),
            offset: query.offset(),
        })
    }
}

#[async_trait::async_trait]
impl DirectoryStore for InMemoryResourceRepository {
    async fn load_all(&self) -> Result<Vec<Directory>, CoreError> {
        Ok(self
            .directories
            .lock()
            .unwrap()
            .values()
            .map(|(directory, _)| directory.clone())
            .collect())
    }

    async fn insert(&self, directory: &Directory) -> Result<(), CoreError> {
        let parent_id = directory
            .parent_id()
            .ok_or_else(|| CoreError::configuration("only the fixed root may lack a parent"))?;
        let parent_path = self
            .directories
            .lock()
            .unwrap()
            .get(&parent_id)
            .map(|(_, path)| path.clone())
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        let path = parent_path.child(directory.name())?;
        self.directories
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
        let current = self
            .directories
            .lock()
            .unwrap()
            .get(&directory.id())
            .cloned();
        if !current.is_some_and(|(current, _)| current.updated_at() == expected_updated_at) {
            return Ok(false);
        }
        self.insert(directory).await?;
        Ok(true)
    }

    async fn remove_if_empty(&self, id: &DirectoryId) -> Result<bool, CoreError> {
        let mut directories = self.directories.lock().unwrap();
        if id.is_root()
            || directories
                .values()
                .any(|(directory, _)| directory.parent_id() == Some(*id))
            || self
                .resources
                .lock()
                .unwrap()
                .values()
                .any(|resource| resource.directory_id() == *id)
        {
            return Ok(false);
        }
        Ok(directories.remove(id).is_some())
    }
}

#[async_trait::async_trait]
impl DirectoryQuery for InMemoryResourceRepository {
    async fn find_by_id(&self, id: &DirectoryId) -> Result<Option<LocatedDirectory>, CoreError> {
        self.directories
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
        self.directories
            .lock()
            .unwrap()
            .iter()
            .find(|(_, (_, candidate))| candidate == path)
            .map(|(id, (directory, _))| {
                LocatedDirectory::new(directory.clone(), DirectoryLocation::new(*id, path.clone()))
            })
            .transpose()
    }

    async fn list_children(
        &self,
        parent_id: &DirectoryId,
    ) -> Result<Vec<LocatedDirectory>, CoreError> {
        let directories = self.directories.lock().unwrap();
        let mut children = directories
            .iter()
            .filter(|(_, (directory, _))| directory.parent_id() == Some(*parent_id))
            .map(|(id, (directory, path))| {
                LocatedDirectory::new(directory.clone(), DirectoryLocation::new(*id, path.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        children.sort_by(|left, right| left.path().cmp(right.path()));
        Ok(children)
    }

    async fn is_descendant_or_self(
        &self,
        ancestor_id: &DirectoryId,
        candidate_id: &DirectoryId,
    ) -> Result<bool, CoreError> {
        let directories = self.directories.lock().unwrap();
        let mut current = Some(*candidate_id);
        while let Some(id) = current {
            if id == *ancestor_id {
                return Ok(true);
            }
            current = directories
                .get(&id)
                .and_then(|(directory, _)| directory.parent_id());
        }
        Ok(false)
    }
}

#[async_trait::async_trait]
impl DirectoryIndex for InMemoryResourceRepository {
    async fn replace_all(&self, directories: Vec<Directory>) -> Result<(), CoreError> {
        let mut indexed = HashMap::new();
        indexed.insert(
            DirectoryId::root(),
            (Directory::root(), DirectoryPath::root()),
        );
        let mut pending = directories
            .into_iter()
            .filter(|directory| !directory.id().is_root())
            .collect::<Vec<_>>();
        while !pending.is_empty() {
            let before = pending.len();
            pending.retain(|directory| {
                let Some(parent_id) = directory.parent_id() else {
                    return true;
                };
                let Some((_, parent_path)) = indexed.get(&parent_id) else {
                    return true;
                };
                let path = parent_path.child(directory.name()).unwrap();
                indexed.insert(directory.id(), (directory.clone(), path));
                false
            });
            if pending.len() == before {
                return Err(CoreError::configuration("invalid directory test index"));
            }
        }
        *self.directories.lock().unwrap() = indexed;
        Ok(())
    }

    async fn upsert(&self, directory: Directory) -> Result<(), CoreError> {
        self.insert(&directory).await
    }

    async fn remove(&self, id: &DirectoryId) -> Result<(), CoreError> {
        self.directories.lock().unwrap().remove(id);
        Ok(())
    }
}

struct InMemoryDirectoryKindRegistry {
    definitions: Vec<DirectoryKindDefinition>,
}

impl Default for InMemoryDirectoryKindRegistry {
    fn default() -> Self {
        Self {
            definitions: vec![DirectoryKindDefinition::with_source(
                crate::domain::DirectoryKind::default(),
                "Directory",
                "test",
            )],
        }
    }
}

impl DirectoryKindRegistry for InMemoryDirectoryKindRegistry {
    fn definitions(&self) -> &[DirectoryKindDefinition] {
        &self.definitions
    }
}

#[derive(Default)]
struct InMemoryResourceKindRegistry {
    definitions: Vec<ResourceKindDefinition>,
}

impl InMemoryResourceKindRegistry {
    fn with_definitions(definitions: Vec<ResourceKindDefinition>) -> Self {
        Self { definitions }
    }
}

impl ResourceKindRegistry for InMemoryResourceKindRegistry {
    fn definitions(&self) -> &[ResourceKindDefinition] {
        &self.definitions
    }
}

struct InMemoryResourceActionRegistry {
    actions: Vec<ResourceActionDefinition>,
}

impl ResourceActionRegistry for InMemoryResourceActionRegistry {
    fn actions(&self) -> &[ResourceActionDefinition] {
        &self.actions
    }
}

struct InMemoryDirectoryActionRegistry {
    actions: Vec<DirectoryActionDefinition>,
}

impl DirectoryActionRegistry for InMemoryDirectoryActionRegistry {
    fn actions(&self) -> &[DirectoryActionDefinition] {
        &self.actions
    }
}

#[derive(Default)]
struct InMemoryBlobStorage {
    objects: Mutex<HashMap<StorageKey, Bytes>>,
    modified_at: Mutex<HashMap<StorageKey, chrono::DateTime<chrono::Utc>>>,
    directories: Mutex<HashSet<DirectoryPath>>,
    fail_next_delete: Mutex<bool>,
    fail_delete_key: Mutex<Option<StorageKey>>,
    fail_scan_after_entries: Mutex<Option<usize>>,
}

impl InMemoryBlobStorage {
    fn contains(&self, key: &StorageKey) -> bool {
        self.objects.lock().unwrap().contains_key(key)
    }

    fn get_sync(&self, key: &StorageKey) -> Option<Bytes> {
        self.objects.lock().unwrap().get(key).cloned()
    }

    fn contains_fragment(&self, fragment: &str) -> bool {
        self.objects
            .lock()
            .unwrap()
            .keys()
            .any(|key| key.as_str().contains(fragment))
    }

    fn fail_next_delete(&self) {
        *self.fail_next_delete.lock().unwrap() = true;
    }

    fn fail_delete_for(&self, key: StorageKey) {
        *self.fail_delete_key.lock().unwrap() = Some(key);
    }

    fn fail_scan_after_entries(&self, entries: usize) {
        *self.fail_scan_after_entries.lock().unwrap() = Some(entries);
    }
}

#[async_trait::async_trait]
impl DirectoryStorage for InMemoryBlobStorage {
    async fn ensure_directory(&self, directory: &DirectoryPath) -> Result<(), CoreError> {
        let mut directories = self.directories.lock().unwrap();
        let mut path = String::new();
        for name in directory.path().split('/').filter(|name| !name.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(name);
            let directory = DirectoryPath::from_path(path.clone())?;
            directories.insert(directory);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl BlobStorage for InMemoryBlobStorage {
    async fn health_check(&self) -> Result<(), CoreError> {
        Ok(())
    }

    async fn put(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
        self.objects.lock().unwrap().insert(key.clone(), data);
        self.modified_at
            .lock()
            .unwrap()
            .insert(key.clone(), chrono::Utc::now());
        Ok(())
    }

    async fn create_staged(&self, key: &StorageKey) -> Result<StagedBlob, CoreError> {
        self.objects
            .lock()
            .unwrap()
            .insert(key.clone(), Bytes::new());
        self.modified_at
            .lock()
            .unwrap()
            .insert(key.clone(), chrono::Utc::now());
        Ok(StagedBlob::new(key.clone(), 0))
    }

    async fn append_staged(
        &self,
        key: &StorageKey,
        expected_offset: u64,
        mut data: BlobByteStream,
    ) -> Result<StagedBlob, CoreError> {
        let actual = self
            .objects
            .lock()
            .unwrap()
            .get(key)
            .map_or(0, |bytes| bytes.len() as u64);
        if actual != expected_offset {
            return Err(CoreError::conflict("upload offset mismatch"));
        }
        while let Some(chunk) = data.next().await {
            let chunk = chunk?;
            let mut objects = self.objects.lock().unwrap();
            let current = objects
                .get(key)
                .cloned()
                .ok_or_else(|| CoreError::not_found("staged upload", key.to_string()))?;
            let mut bytes = current.to_vec();
            bytes.extend_from_slice(&chunk);
            objects.insert(key.clone(), Bytes::from(bytes));
        }
        let size = self.objects.lock().unwrap()[key].len() as u64;
        self.modified_at
            .lock()
            .unwrap()
            .insert(key.clone(), chrono::Utc::now());
        Ok(StagedBlob::new(key.clone(), size))
    }

    async fn inspect_staged(&self, key: &StorageKey) -> Result<Option<StagedBlob>, CoreError> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(key)
            .map(|bytes| StagedBlob::new(key.clone(), bytes.len() as u64)))
    }

    async fn publish_staged_if_absent(
        &self,
        staged: &StagedBlob,
        target: &StorageKey,
    ) -> Result<(), CoreError> {
        let mut objects = self.objects.lock().unwrap();
        if objects.contains_key(target) {
            return Err(CoreError::conflict(format!(
                "storage key `{target}` already exists"
            )));
        }
        let content = objects
            .get(staged.key())
            .cloned()
            .ok_or_else(|| CoreError::not_found("staged blob", staged.key().to_string()))?;
        objects.insert(target.clone(), content);
        self.modified_at
            .lock()
            .unwrap()
            .insert(target.clone(), chrono::Utc::now());
        Ok(())
    }

    async fn discard_staged(&self, staged: &StagedBlob) -> Result<(), CoreError> {
        self.delete(staged.key()).await
    }

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, CoreError> {
        Ok(self.get_sync(key))
    }

    async fn get_stream(&self, key: &StorageKey) -> Result<Option<BlobByteStream>, CoreError> {
        Ok(self.get_sync(key).map(|content| {
            Box::pin(futures_util::stream::once(async move { Ok(content) })) as BlobByteStream
        }))
    }

    async fn get_range_stream(
        &self,
        key: &StorageKey,
        start: u64,
        end: u64,
    ) -> Result<Option<BlobByteStream>, CoreError> {
        Ok(self.get_sync(key).map(|content| {
            let content = content.slice(start as usize..end as usize + 1);
            Box::pin(futures_util::stream::once(async move { Ok(content) })) as BlobByteStream
        }))
    }

    async fn move_if_absent(&self, from: &StorageKey, to: &StorageKey) -> Result<(), CoreError> {
        let mut objects = self.objects.lock().unwrap();
        if objects.contains_key(to) {
            return Err(CoreError::conflict(format!(
                "storage key `{to}` already exists"
            )));
        }
        let content = objects
            .remove(from)
            .ok_or_else(|| CoreError::not_found("blob", from.to_string()))?;
        objects.insert(to.clone(), content);
        let modified_at = self
            .modified_at
            .lock()
            .unwrap()
            .remove(from)
            .unwrap_or_else(chrono::Utc::now);
        self.modified_at
            .lock()
            .unwrap()
            .insert(to.clone(), modified_at);
        Ok(())
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), CoreError> {
        let fail_targeted = self
            .fail_delete_key
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|failed_key| failed_key == key);
        if fail_targeted {
            self.fail_delete_key.lock().unwrap().take();
        }
        if fail_targeted || std::mem::take(&mut *self.fail_next_delete.lock().unwrap()) {
            return Err(CoreError::storage("delete", TestError("delete failed")));
        }
        self.objects.lock().unwrap().remove(key);
        self.modified_at.lock().unwrap().remove(key);
        Ok(())
    }
}

#[async_trait::async_trait]
impl StorageScanner for InMemoryBlobStorage {
    fn scan(&self, prefix: &StoragePrefix) -> crate::port::StorageScanStream {
        let mut directories = self
            .directories
            .lock()
            .unwrap()
            .iter()
            .filter(|directory| {
                prefix.is_root()
                    || directory.path() == prefix.as_str()
                    || directory
                        .path()
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
            .cloned()
            .collect::<Vec<_>>();
        directories.sort();
        let mut entries = directories
            .into_iter()
            .map(ScannedStorageEntry::Directory)
            .collect::<Vec<_>>();
        if prefix.as_str() == crate::port::RESERVED_BLOB_STORAGE_PREFIX
            || prefix
                .as_str()
                .starts_with(&format!("{}/", crate::port::RESERVED_BLOB_STORAGE_PREFIX))
        {
            return Box::pin(futures_util::stream::empty());
        }

        let mut files = self
            .objects
            .lock()
            .unwrap()
            .iter()
            .filter(|(key, _)| {
                if key.as_str() == crate::port::RESERVED_BLOB_STORAGE_PREFIX
                    || key
                        .as_str()
                        .starts_with(&format!("{}/", crate::port::RESERVED_BLOB_STORAGE_PREFIX))
                {
                    return false;
                }
                prefix.contains(key)
            })
            .map(|(key, content)| crate::port::ScannedBlob {
                key: key.clone(),
                size: content.len() as u64,
                mime_type: None,
                modified_at: self.modified_at.lock().unwrap()[key],
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.key.as_str().cmp(right.key.as_str()));
        entries.extend(files.into_iter().map(ScannedStorageEntry::Blob));
        let failure_after = self.fail_scan_after_entries.lock().unwrap().take();
        let mut results = entries.into_iter().map(Ok).collect::<Vec<_>>();
        if let Some(failure_after) = failure_after {
            results.truncate(failure_after);
            results.push(Err(CoreError::storage("scan", TestError("scan failed"))));
        }
        Box::pin(futures_util::stream::iter(results))
    }

    async fn inspect(
        &self,
        key: &StorageKey,
    ) -> Result<Option<crate::port::ScannedBlob>, CoreError> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(key)
            .map(|content| crate::port::ScannedBlob {
                key: key.clone(),
                size: content.len() as u64,
                mime_type: None,
                modified_at: self.modified_at.lock().unwrap()[key],
            }))
    }
}

#[derive(Debug)]
struct TestError(&'static str);

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for TestError {}

#[derive(Debug, Default)]
struct StaticResourceActionExecutor;

#[async_trait]
impl ResourceActionExecutor for StaticResourceActionExecutor {
    async fn execute(
        &self,
        request: ResourceActionRequest,
    ) -> Result<ResourceActionOutput, CoreError> {
        let view = match request.action().as_str() {
            "test.text.extract" => PluginView::Text(TextView {
                text: String::from_utf8(
                    request
                        .content()
                        .map(|content| content.to_vec())
                        .unwrap_or_default(),
                )
                .unwrap(),
            }),
            "azvs.markdown.edit" => {
                let markdown = request
                    .input()
                    .get("markdown")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string();
                let mut output = PluginActionOutput::new(PluginView::Text(TextView {
                    text: "saved".to_string(),
                }));
                output
                    .effects
                    .push(PluginActionEffect::ReplaceContent(ReplaceContentEffect {
                        encoding: PluginReplacementEncoding::Base64,
                        data: STANDARD.encode(markdown),
                        mime_type: Some("text/markdown".to_string()),
                    }));

                return Ok(ResourceActionOutput::new(
                    request.resource().id(),
                    request.action().clone(),
                    output,
                ));
            }
            action => {
                return Err(CoreError::configuration(format!(
                    "unexpected test action `{action}`"
                )));
            }
        };

        Ok(ResourceActionOutput::new(
            request.resource().id(),
            request.action().clone(),
            PluginActionOutput::new(view),
        ))
    }
}

#[derive(Debug, Default)]
struct StaticDirectoryActionExecutor;

#[async_trait]
impl DirectoryActionExecutor for StaticDirectoryActionExecutor {
    async fn execute(
        &self,
        request: DirectoryActionRequest,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let parent_id = request
            .input()
            .get("parent_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let mut output = DirectoryPluginActionOutput::new(PluginView::Text(TextView {
            text: "updated".to_string(),
        }));
        output
            .effects
            .push(DirectoryActionEffect::Update(UpdateDirectoryEffect {
                name: None,
                parent_id,
                kind: None,
            }));
        Ok(DirectoryActionOutput::new(
            request.directory().id(),
            request.action().clone(),
            output,
        ))
    }
}

struct SingleUserRepository(User);

#[async_trait]
impl UserRepository for SingleUserRepository {
    async fn create(&self, _user: &User) -> Result<(), CoreError> {
        Ok(())
    }

    async fn save(&self, _user: &User) -> Result<(), CoreError> {
        Ok(())
    }

    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError> {
        Ok((self.0.id() == *id).then(|| self.0.clone()))
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError> {
        Ok((self.0.username() == username).then(|| self.0.clone()))
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);

    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("test future unexpectedly returned pending"),
    }
}

fn service() -> (
    ResourceService,
    Arc<InMemoryResourceRepository>,
    Arc<InMemoryBlobStorage>,
) {
    let kind_registry = Arc::new(InMemoryResourceKindRegistry::with_definitions(vec![
        ResourceKindDefinition::new(ResourceKind::default(), "Resource", true),
        ResourceKindDefinition::new(
            ResourceKind::try_new("doc:markdown").unwrap(),
            "Markdown",
            false,
        ),
        ResourceKindDefinition::new(ResourceKind::try_new("core:image").unwrap(), "Image", true),
        ResourceKindDefinition::new(
            ResourceKind::try_new("asset:binary").unwrap(),
            "Binary",
            true,
        ),
        ResourceKindDefinition::new(ResourceKind::try_new("core:text").unwrap(), "Text", true),
        ResourceKindDefinition::new(
            ResourceKind::try_new("azvs:markdown").unwrap(),
            "Markdown",
            true,
        )
        .with_parent(Some(ResourceKind::try_new("core:text").unwrap()))
        .with_detect(
            ResourceContentMatcher::new()
                .with_mime_types(["text/markdown", "text/x-markdown"])
                .with_extensions([".md", ".markdown"]),
        ),
        ResourceKindDefinition::new(ResourceKind::try_new("core:video").unwrap(), "Video", true),
    ]));
    let action_registry = Arc::new(InMemoryResourceActionRegistry {
        actions: vec![
            ResourceActionDefinition::new("test.text.extract", "Extract text")
                .with_kinds(["doc:markdown", "core:text"])
                .with_requirements(content_requirements())
                .with_output(output_contract(["text"])),
            ResourceActionDefinition::new("resource.inspect", "Inspect resource")
                .with_kinds(["doc:markdown"])
                .with_output(output_contract(["json"])),
            ResourceActionDefinition::new("doc.markdown.thumbnail", "Markdown thumbnail")
                .with_provides(Some("thumbnail"))
                .with_kinds(["doc:markdown"])
                .with_requirements(content_requirements())
                .with_output(output_contract(["media"])),
            ResourceActionDefinition::new("test.text.thumbnail", "Text thumbnail")
                .with_provides(Some("thumbnail"))
                .with_kinds(["core:text"])
                .with_content_matcher(
                    ResourceContentMatcher::new().with_mime_types(["application/pdf"]),
                )
                .with_output(output_contract(["media"])),
            ResourceActionDefinition::new("core.resource.thumbnail", "Thumbnail")
                .with_provides(Some("thumbnail"))
                .with_output(output_contract(["media"])),
            ResourceActionDefinition::new("azvs.markdown.read", "Read Markdown")
                .with_provides(Some("text_read"))
                .with_kinds(["core:text"])
                .with_requirements(content_requirements())
                .with_output(output_contract(["plugin_frame"]))
                .with_content_matcher(
                    ResourceContentMatcher::new()
                        .with_mime_types(["text/markdown", "text/x-markdown"])
                        .with_extensions([".md", ".markdown"]),
                ),
            ResourceActionDefinition::new("azvs.markdown.edit", "Edit Markdown")
                .with_provides(Some("text_edit"))
                .with_kinds(["core:text"])
                .with_requirements(content_requirements())
                .with_access(ResourceActionAccess::ReadWrite)
                .with_output(output_contract(["text"]))
                .with_content_matcher(
                    ResourceContentMatcher::new()
                        .with_mime_types(["text/markdown", "text/x-markdown"])
                        .with_extensions([".md", ".markdown"]),
                ),
        ],
    });
    let repository = Arc::new(InMemoryResourceRepository::default());
    let blob_storage = Arc::new(InMemoryBlobStorage::default());
    let service = ResourceService::new(
        ResourceServicePorts::new(
            repository.clone(),
            repository.clone(),
            blob_storage.clone(),
            repository.clone(),
            repository.clone(),
            blob_storage.clone(),
            Arc::new(InMemoryDirectoryKindRegistry::default()),
            blob_storage.clone(),
            kind_registry,
            Arc::new(InMemoryUploadSessionRepository::default()),
            Arc::new(InMemoryContentReplacementRepository::default()),
        )
        .with_actions(action_registry, Arc::new(StaticResourceActionExecutor))
        .with_directory_actions(
            Arc::new(InMemoryDirectoryActionRegistry {
                actions: vec![
                    DirectoryActionDefinition::new("test.directory.move", "Move directory")
                        .with_kinds(["core:directory"])
                        .with_access(DirectoryActionAccess::ReadWrite)
                        .with_output(output_contract(["text"])),
                ],
            }),
            Arc::new(StaticDirectoryActionExecutor),
        ),
        Arc::new(test_resource_action_policy()),
        Arc::new(test_resource_content_edit_policy()),
    );

    (service, repository, blob_storage)
}

#[test]
fn directory_action_cannot_move_a_directory_outside_the_member_workspace() {
    let (service, _, _) = service();
    let root = block_on(service.directory_service().root()).unwrap();
    let workspace = block_on(service.directory_service().create(&root, "workspace")).unwrap();
    let outside = block_on(service.directory_service().create(&root, "outside")).unwrap();
    let inside = block_on(service.directory_service().create(&workspace, "inside")).unwrap();
    let user = User::new("member", "hash", UserRole::Member, workspace.id()).unwrap();
    let context = AccessContext::member(user.id());
    let authorization = crate::service::AuthorizationService::new(
        Arc::new(SingleUserRepository(user)),
        service.directory_service().clone(),
    );

    let error = block_on(
        service
            .secured(&authorization, &context)
            .execute_directory_action(
                &inside.id(),
                crate::service::ExecuteDirectoryAction::new("test.directory.move")
                    .with_input(serde_json::json!({"parent_id": outside.id().to_string()})),
            ),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::Forbidden { .. }));
    assert_eq!(
        block_on(service.directory_service().locate_by_id(&inside.id()))
            .unwrap()
            .path()
            .path(),
        "workspace/inside"
    );
}

#[test]
fn storage_reconciliation_creates_updates_and_removes_resources() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::from_path("external").unwrap();
    let key = StorageKey::new("external/note.txt").unwrap();
    block_on(blob_storage.ensure_directory(&directory)).unwrap();
    block_on(blob_storage.put(&key, Bytes::from_static(b"first"))).unwrap();

    block_on(service.reconcile_storage()).unwrap();
    let first = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        "note.txt",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(first.resource().content().unwrap().size(), 5);

    block_on(blob_storage.put(&key, Bytes::from_static(b"second version"))).unwrap();
    block_on(service.reconcile_storage_keys(std::slice::from_ref(&key))).unwrap();
    let updated = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        "note.txt",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(updated.resource().id(), first.resource().id());
    assert_eq!(updated.resource().content().unwrap().size(), 14);
    assert_ne!(
        updated.resource().content().unwrap().checksum(),
        first.resource().content().unwrap().checksum()
    );

    block_on(blob_storage.delete(&key)).unwrap();
    block_on(service.reconcile_storage_keys(std::slice::from_ref(&key))).unwrap();
    assert!(
        block_on(ResourceQuery::find_by_path(
            repository.as_ref(),
            &directory,
            "note.txt",
        ))
        .unwrap()
        .is_none()
    );
}

#[test]
fn startup_reconciliation_hashes_only_new_or_changed_files() {
    let (service, _, blob_storage) = service();
    let key = StorageKey::new("library/book.txt").unwrap();
    block_on(blob_storage.put(&key, Bytes::from_static(b"first"))).unwrap();

    let initial = block_on(service.reconcile_storage()).unwrap();
    assert_eq!(initial.files, 1);
    assert_eq!(initial.hashed_files, 1);
    assert_eq!(initial.unchanged_files, 0);

    let unchanged = block_on(service.reconcile_storage()).unwrap();
    assert_eq!(unchanged.hashed_files, 0);
    assert_eq!(unchanged.unchanged_files, 1);

    block_on(blob_storage.put(&key, Bytes::from_static(b"other"))).unwrap();
    let changed = block_on(service.reconcile_storage()).unwrap();
    assert_eq!(changed.hashed_files, 1);
    assert_eq!(changed.unchanged_files, 0);

    let forced = block_on(service.scan_resources()).unwrap();
    assert_eq!(forced.hashed_files, 1);
    assert_eq!(forced.unchanged_files, 0);
}

#[test]
fn empty_repository_startup_recovers_metadata_before_checksum_verification() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::from_path("library").unwrap();
    let key = StorageKey::new("library/book.txt").unwrap();
    let second_key = StorageKey::new("library/second.txt").unwrap();
    block_on(blob_storage.put(&key, Bytes::from_static(b"first"))).unwrap();
    block_on(blob_storage.put(&second_key, Bytes::from_static(b"second"))).unwrap();

    let recovered = block_on(service.reconcile_storage_on_startup()).unwrap();
    assert_eq!(recovered.files, 2);
    assert_eq!(recovered.hashed_files, 0);
    assert_eq!(
        recovered.pending_verification_keys(),
        &[key.clone(), second_key.clone()]
    );

    let pending = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        "book.txt",
    ))
    .unwrap()
    .unwrap();
    let content = pending.resource().content().unwrap();
    assert_eq!(
        content.verification_status(),
        ContentVerificationStatus::Pending
    );
    assert_eq!(content.size(), 5);
    assert_eq!(content.checksum(), None);

    block_on(service.reconcile_storage_keys(std::slice::from_ref(&key))).unwrap();
    let verified = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        "book.txt",
    ))
    .unwrap()
    .unwrap();
    let content = verified.resource().content().unwrap();
    assert_eq!(
        content.verification_status(),
        ContentVerificationStatus::Verified
    );
    assert_eq!(content.checksum().unwrap().value(), hex_sha256(b"first"));

    let resumed = block_on(service.reconcile_storage_on_startup()).unwrap();
    assert_eq!(resumed.hashed_files, 0);
    assert_eq!(resumed.unchanged_files, 1);
    assert_eq!(resumed.pending_verification_keys(), &[second_key]);
}

#[test]
fn storage_reconciliation_preserves_spaces_in_discovered_paths() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::from_path(" external files / project A ").unwrap();
    let name = " draft  01.txt ";
    let key = StorageKey::new(" external files / project A / draft  01.txt ").unwrap();
    block_on(blob_storage.ensure_directory(&directory)).unwrap();
    block_on(blob_storage.put(&key, Bytes::from_static(b"draft"))).unwrap();

    block_on(service.reconcile_storage()).unwrap();

    let resource = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        name,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(resource.resource().name(), name);
    assert_eq!(
        block_on(service.locate_resource_directory(resource.resource()))
            .unwrap()
            .path(),
        &directory
    );
    assert_eq!(
        block_on(service.storage_key(resource.resource())).unwrap(),
        key
    );
    assert_eq!(
        block_on(
            service
                .content()
                .get_resource_content(&resource.resource().id())
        )
        .unwrap(),
        Some(Bytes::from_static(b"draft"))
    );
}

#[test]
fn storage_reconciliation_preserves_resource_id_on_file_rename() {
    let (service, repository, blob_storage) = service();
    let from_directory = DirectoryPath::from_path("incoming").unwrap();
    let to_directory = DirectoryPath::from_path("library").unwrap();
    let from = StorageKey::new("incoming/book.txt").unwrap();
    let to = StorageKey::new("library/renamed.txt").unwrap();
    block_on(blob_storage.ensure_directory(&from_directory)).unwrap();
    block_on(blob_storage.ensure_directory(&to_directory)).unwrap();
    block_on(blob_storage.put(&from, Bytes::from_static(b"content"))).unwrap();
    block_on(service.reconcile_storage()).unwrap();
    let original = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &from_directory,
        "book.txt",
    ))
    .unwrap()
    .unwrap();

    block_on(blob_storage.move_if_absent(&from, &to)).unwrap();
    block_on(service.reconcile_storage_rename(&from, &to)).unwrap();
    let renamed = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &to_directory,
        "renamed.txt",
    ))
    .unwrap()
    .unwrap();

    assert_eq!(renamed.resource().id(), original.resource().id());
    assert!(
        block_on(ResourceQuery::find_by_path(
            repository.as_ref(),
            &from_directory,
            "book.txt",
        ))
        .unwrap()
        .is_none()
    );
}

#[test]
fn storage_reconciliation_does_not_delete_resources_after_stream_failure() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::root();
    let retained = StorageKey::new("a.txt").unwrap();
    let externally_deleted = StorageKey::new("b.txt").unwrap();
    block_on(blob_storage.put(&retained, Bytes::from_static(b"a"))).unwrap();
    block_on(blob_storage.put(&externally_deleted, Bytes::from_static(b"b"))).unwrap();
    block_on(service.reconcile_storage()).unwrap();

    block_on(blob_storage.delete(&externally_deleted)).unwrap();
    blob_storage.fail_scan_after_entries(1);
    assert!(block_on(service.reconcile_storage()).is_err());

    assert!(
        block_on(ResourceQuery::find_by_path(
            repository.as_ref(),
            &directory,
            "b.txt",
        ))
        .unwrap()
        .is_some()
    );
}

fn content_requirements() -> crate::domain::ResourceActionRequirements {
    crate::domain::ResourceActionRequirements {
        content: true,
        content_delivery: crate::domain::ResourceActionContentDelivery::Inline,
    }
}

fn test_resource_action_policy() -> ResourceActionPolicy {
    ResourceActionPolicy::new(64 * 1024 * 1024, 4 * 1024 * 1024).unwrap()
}

fn test_resource_content_edit_policy() -> ResourceContentEditPolicy {
    ResourceContentEditPolicy::new(4 * 1024 * 1024).unwrap()
}

fn output_contract<const N: usize>(
    views: [&str; N],
) -> crate::domain::ResourceActionOutputContract {
    crate::domain::ResourceActionOutputContract {
        view: views.into_iter().map(str::to_string).collect(),
    }
}

struct TestUpload {
    name: String,
    kind: Option<ResourceKind>,
    directory: DirectoryPath,
    tags: Vec<String>,
    content: BlobByteStream,
    mime_type: Option<String>,
}

impl TestUpload {
    fn new(name: impl Into<String>, content: BlobByteStream) -> Self {
        Self {
            name: name.into(),
            kind: None,
            directory: DirectoryPath::root(),
            tags: Vec::new(),
            content,
            mime_type: None,
        }
    }

    fn with_kind(mut self, kind: ResourceKind) -> Self {
        self.kind = Some(kind);
        self
    }

    fn with_directory(mut self, directory: DirectoryPath) -> Self {
        self.directory = directory;
        self
    }

    fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}

impl ResourceService {
    async fn upload_resource_for_test(&self, draft: TestUpload) -> Result<Resource, CoreError> {
        let TestUpload {
            name,
            kind,
            directory,
            tags,
            mut content,
            mime_type,
        } = draft;
        let mut bytes = Vec::new();
        while let Some(chunk) = content.next().await {
            bytes.extend_from_slice(&chunk?);
        }
        let owner = UserId::new();
        let expected_checksum = Checksum::sha256(hex_sha256(&bytes)).unwrap();
        let mut command = CreateUpload::new(name, bytes.len() as u64, expected_checksum.clone())
            .with_directory(directory)
            .with_tags(tags);
        if let Some(kind) = kind {
            command = command.with_kind(kind);
        }
        if let Some(mime_type) = mime_type {
            command = command.with_mime_type(mime_type);
        }
        let session = self.uploads().create(owner, command).await?;
        let data = futures_util::stream::once(async move { Ok(Bytes::from(bytes)) });
        self.uploads()
            .append(owner, &session.id(), 0, expected_checksum, Box::pin(data))
            .await?;
        let (session, should_finalize) = self
            .uploads()
            .request_finalization(owner, &session.id())
            .await?;
        if !should_finalize {
            return Err(CoreError::conflict(
                "test upload did not enter finalization",
            ));
        }
        self.uploads().finalize(&session.id()).await
    }
}

fn stream_upload_command(
    _name: impl Into<String>,
    storage_key: StorageKey,
    data: Bytes,
) -> TestUpload {
    let (directory, name) = storage_key
        .as_str()
        .rsplit_once('/')
        .unwrap_or(("", storage_key.as_str()));
    let stream = futures_util::stream::once(async move { Ok(data) });
    TestUpload::new(name, Box::pin(stream))
        .with_directory(DirectoryPath::from_path(directory).unwrap())
}

#[test]
fn action_content_delivery_never_loads_unrequested_content() {
    use crate::domain::{ResourceActionContentDelivery, ResourceActionRequirements};
    let policy = test_resource_action_policy();

    let without_content = ResourceActionDefinition::new("inspect", "Inspect");
    assert_eq!(
        resolved_content_delivery(&without_content, 1, &policy),
        None
    );

    let required = |delivery| {
        ResourceActionDefinition::new("inspect", "Inspect").with_requirements(
            ResourceActionRequirements {
                content: true,
                content_delivery: delivery,
            },
        )
    };
    assert_eq!(
        resolved_content_delivery(
            &required(ResourceActionContentDelivery::Inline),
            policy.max_inline_content_bytes() + 1,
            &policy,
        ),
        Some(ResourceActionContentDelivery::Inline)
    );
    assert_eq!(
        resolved_content_delivery(
            &required(ResourceActionContentDelivery::Reference),
            1,
            &policy,
        ),
        Some(ResourceActionContentDelivery::Reference)
    );
    assert_eq!(
        resolved_content_delivery(
            &required(ResourceActionContentDelivery::Auto),
            policy.max_inline_content_bytes(),
            &policy,
        ),
        Some(ResourceActionContentDelivery::Inline)
    );
    assert_eq!(
        resolved_content_delivery(
            &required(ResourceActionContentDelivery::Auto),
            policy.max_inline_content_bytes() + 1,
            &policy,
        ),
        Some(ResourceActionContentDelivery::Reference)
    );
}

fn service_with_registry(
    kind_registry: Arc<dyn ResourceKindRegistry>,
) -> (
    ResourceService,
    Arc<InMemoryResourceRepository>,
    Arc<InMemoryBlobStorage>,
) {
    let repository = Arc::new(InMemoryResourceRepository::default());
    let blob_storage = Arc::new(InMemoryBlobStorage::default());
    let service = ResourceService::new(
        ResourceServicePorts::new(
            repository.clone(),
            repository.clone(),
            blob_storage.clone(),
            repository.clone(),
            repository.clone(),
            blob_storage.clone(),
            Arc::new(InMemoryDirectoryKindRegistry::default()),
            blob_storage.clone(),
            kind_registry,
            Arc::new(InMemoryUploadSessionRepository::default()),
            Arc::new(InMemoryContentReplacementRepository::default()),
        ),
        Arc::new(test_resource_action_policy()),
        Arc::new(test_resource_content_edit_policy()),
    );

    (service, repository, blob_storage)
}

#[test]
fn update_resource_rejects_a_stale_authorized_snapshot() {
    let (service, repository, _) = service();
    let resource = command::build_resource(
        "original".to_string(),
        DirectoryId::root(),
        Some(ResourceKind::try_new("doc:markdown").unwrap()),
        Vec::new(),
    )
    .build()
    .unwrap();
    block_on(repository.save(&resource)).unwrap();
    let stale = resource.clone();
    let mut concurrent = resource;
    concurrent.rename("concurrent").unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(service.commands().update_resource_snapshot(
        repository.locate_sync(stale),
        UpdateResource::new().with_name("stale"),
    ))
    .unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(
        repository.find_sync(&concurrent.id()).unwrap().name(),
        "concurrent"
    );
}

#[test]
fn resource_without_content_describes_only_actions_without_content_requirements() {
    let (service, _, _) = service();
    let resource = command::build_resource(
        "contentless".to_string(),
        DirectoryId::root(),
        Some(ResourceKind::try_new("doc:markdown").unwrap()),
        Vec::new(),
    )
    .build()
    .unwrap();

    let actions = service
        .actions()
        .describe_resource_actions(&resource)
        .unwrap();
    let ids = actions
        .available_actions()
        .iter()
        .map(|action| action.id().as_str())
        .collect::<Vec<_>>();

    assert_eq!(ids, vec!["resource.inspect", "core.resource.thumbnail"]);
}

#[test]
fn resource_without_content_rejects_direct_content_action_execution() {
    let (service, repository, _) = service();
    let resource = command::build_resource(
        "contentless".to_string(),
        DirectoryId::root(),
        Some(ResourceKind::try_new("doc:markdown").unwrap()),
        Vec::new(),
    )
    .build()
    .unwrap();
    block_on(repository.save(&resource)).unwrap();

    let error = block_on(service.actions().execute_resource_action(
        &resource.id(),
        ExecuteResourceAction::new("test.text.extract"),
    ))
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("does not support action `test.text.extract`")
    );
}

#[test]
fn stream_upload_resource_content_writes_blob_then_saves_resource() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let checksum = Checksum::sha256(hex_sha256(&data)).unwrap();

    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("image", key.clone(), data.clone())
                .with_kind(ResourceKind::try_new("core:image").unwrap())
                .with_mime_type(" image/png "),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();
    let content = saved.content().unwrap();

    assert_eq!(block_on(service.storage_key(&saved)).unwrap(), key);
    assert_eq!(content.size(), data.len() as u64);
    assert_eq!(content.mime_type(), Some("image/png"));
    assert_eq!(content.checksum(), Some(&checksum));
    assert_eq!(
        content.modified_at(),
        blob_storage.modified_at.lock().unwrap().get(&key).copied()
    );
    assert_eq!(blob_storage.get_sync(&key), Some(data));

    let startup = block_on(service.reconcile_storage()).unwrap();
    assert_eq!(startup.hashed_files, 0);
    assert_eq!(startup.unchanged_files, 1);
}

#[test]
fn stream_upload_preserves_spaces_in_resource_and_blob_path() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::from_path(" library / project A ").unwrap();
    let name = " design  draft 01.md ";
    let key = StorageKey::new(" library / project A / design  draft 01.md ").unwrap();
    let data = Bytes::from_static(b"draft");
    let stream = futures_util::stream::once({
        let data = data.clone();
        async move { Ok(data) }
    });

    let resource = block_on(
        service.upload_resource_for_test(
            TestUpload::new(name, Box::pin(stream))
                .with_directory(directory.clone())
                .with_kind(ResourceKind::try_new("azvs:markdown").unwrap()),
        ),
    )
    .unwrap();

    assert_eq!(resource.name(), name);
    assert_eq!(
        block_on(service.locate_resource_directory(&resource))
            .unwrap()
            .path(),
        &directory
    );
    assert_eq!(block_on(service.storage_key(&resource)).unwrap(), key);
    assert_eq!(
        block_on(service.storage_key(&repository.find_sync(&resource.id()).unwrap())).unwrap(),
        key
    );
    assert_eq!(
        blob_storage.get_sync(&key),
        Some(Bytes::from_static(b"draft"))
    );
}

#[test]
fn stream_upload_resource_content_detects_most_specific_kind() {
    let (service, repository, _) = service();
    let key = StorageKey::new("docs/readme.md").unwrap();

    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("readme", key, Bytes::from_static(b"# Readme"))
                .with_mime_type("text/plain"),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();

    assert!(saved.kind().is("azvs:markdown"));
}

#[test]
fn stream_upload_resource_content_falls_back_to_core_resource() {
    let (service, repository, _) = service();
    let key = StorageKey::new("assets/archive.bin").unwrap();

    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("archive", key, Bytes::from_static(b"binary"))
                .with_mime_type("application/octet-stream"),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();
    assert_eq!(saved.kind(), &ResourceKind::default());
}

#[test]
fn stream_upload_resource_content_rejects_existing_storage_key() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    blob_storage
        .objects
        .lock()
        .unwrap()
        .insert(key.clone(), Bytes::from_static(b"existing"));
    blob_storage
        .modified_at
        .lock()
        .unwrap()
        .insert(key.clone(), chrono::Utc::now());

    let error = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key,
        Bytes::from_static(b"new"),
    )))
    .unwrap_err();

    match error {
        CoreError::Conflict { message } => assert!(message.contains("already exists")),
        other => panic!("expected storage key conflict, got {other:?}"),
    }
    assert!(repository.is_empty());
    assert_eq!(
        blob_storage.get_sync(&StorageKey::new("assets/image.png").unwrap()),
        Some(Bytes::from_static(b"existing"))
    );
    assert!(blob_storage.contains_fragment(".asset-hub/uploads/"));
}

#[test]
fn stream_upload_rejects_unsupported_kind() {
    let (service, repository, _) = service_with_registry(Arc::new(
        InMemoryResourceKindRegistry::with_definitions(vec![ResourceKindDefinition::new(
            ResourceKind::default(),
            "Unknown",
            true,
        )]),
    ));

    let error = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "image",
                StorageKey::new("image.png").unwrap(),
                Bytes::from_static(b"image"),
            )
            .with_kind(ResourceKind::try_new("plugin:not-installed").unwrap()),
        ),
    )
    .unwrap_err();

    match error {
        CoreError::Configuration { message } => {
            assert!(message.contains("unsupported resource kind `plugin:not-installed`"))
        }
        other => panic!("expected configuration error, got {other:?}"),
    }
    assert!(repository.is_empty());
}

#[test]
fn stream_create_resource_writes_chunks_and_records_size() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/large.bin").unwrap();
    let data: BlobByteStream = Box::pin(futures_util::stream::iter([
        Ok(Bytes::from_static(b"large ")),
        Ok(Bytes::from_static(b"file ")),
        Ok(Bytes::from_static(b"bytes")),
    ]));

    let resource = block_on(
        service.upload_resource_for_test(
            TestUpload::new("large.bin", data)
                .with_directory(DirectoryPath::from_path("assets").unwrap())
                .with_kind(ResourceKind::try_new("asset:binary").unwrap())
                .with_mime_type("application/octet-stream"),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();
    let content = saved.content().unwrap();

    assert_eq!(block_on(service.storage_key(&saved)).unwrap(), key);
    assert_eq!(content.size(), 16);
    assert_eq!(content.mime_type(), Some("application/octet-stream"));
    assert_eq!(
        blob_storage.get_sync(&key),
        Some(Bytes::from_static(b"large file bytes"))
    );
    assert!(!blob_storage.contains_fragment(".asset-hub/uploads/"));
}

#[tokio::test]
async fn pending_upload_finalization_is_resumed_in_the_background() {
    let (service, repository, blob_storage) = service();
    let owner = UserId::new();
    let data = Bytes::from_static(b"resume finalization");
    let session = service
        .uploads()
        .create(
            owner,
            CreateUpload::new(
                "resumed.bin",
                data.len() as u64,
                Checksum::sha256(hex_sha256(&data)).unwrap(),
            )
            .with_directory(DirectoryPath::from_path("assets").unwrap()),
        )
        .await
        .unwrap();
    service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(&data)).unwrap(),
            Box::pin(futures_util::stream::once({
                let data = data.clone();
                async move { Ok(data) }
            })),
        )
        .await
        .unwrap();
    let (finalizing, should_start) = service
        .uploads()
        .request_finalization(owner, &session.id())
        .await
        .unwrap();
    assert!(should_start);
    assert_eq!(finalizing.status(), UploadStatus::Finalizing);

    assert_eq!(service.resume_upload_finalizations().await.unwrap(), 1);
    let resource = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if let Some(resource) = repository.find_sync(&session.resource_id()) {
                break resource;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("resumed finalization should finish");

    assert_eq!(resource.id(), session.resource_id());
    assert_eq!(
        blob_storage.get_sync(&StorageKey::new("assets/resumed.bin").unwrap()),
        Some(data)
    );
}

#[tokio::test]
async fn storage_reconciliation_waits_for_same_key_upload_to_save_resource() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/slow.bin").unwrap();
    let (save_started, release_save) = repository.pause_next_save();
    let upload_service = service.clone();
    let upload_key = key.clone();
    let upload = tokio::spawn(async move {
        upload_service
            .upload_resource_for_test(stream_upload_command(
                "slow.bin",
                upload_key,
                Bytes::from_static(b"complete content"),
            ))
            .await
    });

    save_started
        .await
        .expect("upload should reach repository save");
    assert!(blob_storage.contains(&key));
    assert!(repository.is_empty());

    let reconcile_service = service.clone();
    let reconcile_key = key.clone();
    let reconciliation = tokio::spawn(async move {
        reconcile_service
            .reconcile_storage_keys(&[reconcile_key])
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !reconciliation.is_finished(),
        "same-key reconciliation must wait until upload saves the Resource"
    );

    release_save.send(()).unwrap();
    let uploaded = upload.await.unwrap().unwrap();
    reconciliation.await.unwrap().unwrap();

    assert_eq!(repository.len(), 1);
    assert_eq!(
        repository.find_sync(&uploaded.id()).unwrap().content(),
        uploaded.content()
    );
}

#[test]
fn stream_upload_resource_content_rejects_kind_without_content_support() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/readme.md").unwrap();

    let error = block_on(
        service.upload_resource_for_test(
            stream_upload_command("readme", key.clone(), Bytes::from_static(b"hello"))
                .with_kind(ResourceKind::try_new("doc:markdown").unwrap()),
        ),
    )
    .unwrap_err();

    match error {
        CoreError::Configuration { message } => {
            assert!(message.contains("does not support content upload"))
        }
        other => panic!("expected configuration error, got {other:?}"),
    }
    assert!(repository.is_empty());
    assert!(!blob_storage.contains(&key));
}

#[test]
fn stream_upload_resource_content_removes_blob_when_save_fails() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    repository.fail_next_save();

    let result = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key.clone(),
        Bytes::from_static(b"image bytes"),
    )));

    match result {
        Err(CoreError::Repository { operation, .. }) => assert_eq!(operation, "save"),
        other => panic!("expected repository error, got {other:?}"),
    }

    assert!(!blob_storage.contains(&key));
    assert!(blob_storage.contains_fragment(".asset-hub/uploads/"));
    assert!(repository.is_empty());
}

#[test]
fn upload_preserves_repository_error_when_compensation_delete_fails() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/compensation.bin").unwrap();
    repository.fail_next_save();
    blob_storage.fail_delete_for(key.clone());

    let error = block_on(service.upload_resource_for_test(stream_upload_command(
        "file",
        key.clone(),
        Bytes::from_static(b"data"),
    )))
    .unwrap_err();

    assert!(matches!(
        error,
        CoreError::Repository {
            operation: "save",
            ..
        }
    ));
    assert!(blob_storage.contains(&key));
    assert!(blob_storage.contains_fragment(".asset-hub/uploads/"));
}

#[tokio::test]
async fn upload_chunk_checksum_mismatch_keeps_offset_and_staged_content_unchanged() {
    let (service, _repository, blob_storage) = service();
    let owner = UserId::new();
    let data = Bytes::from_static(b"verified chunk");
    let session = service
        .uploads()
        .create(
            owner,
            CreateUpload::new(
                "chunk.bin",
                data.len() as u64,
                Checksum::sha256(hex_sha256(&data)).unwrap(),
            ),
        )
        .await
        .unwrap();

    let error = service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(b"different chunk")).unwrap(),
            Box::pin(futures_util::stream::once({
                let data = data.clone();
                async move { Ok(data) }
            })),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("chunk checksum mismatch"));
    let status = service
        .uploads()
        .status(owner, &session.id())
        .await
        .unwrap();
    assert_eq!(status.offset(), 0);
    let staged_key = StorageKey::new(format!(".asset-hub/uploads/{}", session.id())).unwrap();
    assert_eq!(blob_storage.get_sync(&staged_key), Some(Bytes::new()));
    assert!(!blob_storage.contains_fragment(".chunk"));

    let resumed = service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(&data)).unwrap(),
            Box::pin(futures_util::stream::once(async move { Ok(data) })),
        )
        .await
        .unwrap();
    assert_eq!(resumed.offset(), resumed.expected_size());
    assert_eq!(
        blob_storage.get_sync(&staged_key),
        Some(Bytes::from_static(b"verified chunk"))
    );
    assert!(!blob_storage.contains_fragment(".chunk"));
}

#[tokio::test]
async fn interrupted_upload_chunk_never_reaches_the_session_staging_file() {
    let (service, _repository, blob_storage) = service();
    let owner = UserId::new();
    let session = service
        .uploads()
        .create(
            owner,
            CreateUpload::new(
                "interrupted.bin",
                12,
                Checksum::sha256(hex_sha256(b"partial data")).unwrap(),
            ),
        )
        .await
        .unwrap();
    let stream = futures_util::stream::iter(vec![
        Ok(Bytes::from_static(b"partial")),
        Err(CoreError::conflict("request body interrupted")),
    ]);

    let error = service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(b"partial data")).unwrap(),
            Box::pin(stream),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("request body interrupted"));
    let status = service
        .uploads()
        .status(owner, &session.id())
        .await
        .unwrap();
    assert_eq!(status.offset(), 0);
    let staged_key = StorageKey::new(format!(".asset-hub/uploads/{}", session.id())).unwrap();
    assert_eq!(blob_storage.get_sync(&staged_key), Some(Bytes::new()));
    assert!(!blob_storage.contains_fragment(".chunk"));
}

#[tokio::test]
async fn upload_checksum_mismatch_fails_before_publication() {
    let (service, repository, blob_storage) = service();
    let owner = UserId::new();
    let expected = Bytes::from_static(b"right");
    let received = Bytes::from_static(b"wrong");
    let key = StorageKey::new("assets/mismatch.bin").unwrap();
    let session = service
        .uploads()
        .create(
            owner,
            CreateUpload::new(
                "mismatch.bin",
                received.len() as u64,
                Checksum::sha256(hex_sha256(&expected)).unwrap(),
            )
            .with_directory(DirectoryPath::from_path("assets").unwrap()),
        )
        .await
        .unwrap();
    service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(&received)).unwrap(),
            Box::pin(futures_util::stream::once(async move { Ok(received) })),
        )
        .await
        .unwrap();
    service
        .uploads()
        .request_finalization(owner, &session.id())
        .await
        .unwrap();

    let error = service.uploads().finalize(&session.id()).await.unwrap_err();
    assert!(error.to_string().contains("checksum mismatch"));
    let failed = service
        .uploads()
        .status(owner, &session.id())
        .await
        .unwrap();
    assert_eq!(failed.status(), UploadStatus::Failed);
    assert_eq!(
        failed.actual_checksum().unwrap().value(),
        hex_sha256(b"wrong")
    );
    assert!(repository.find_sync(&session.resource_id()).is_none());
    assert!(!blob_storage.contains(&key));
    assert!(blob_storage.contains_fragment(".asset-hub/uploads/"));
}

#[test]
fn get_resource_content_reads_existing_blob() {
    let (service, _, _) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key,
        data.clone(),
    )))
    .unwrap();

    let content = block_on(service.content().get_resource_content(&resource.id())).unwrap();

    assert_eq!(content, Some(data));
}

#[test]
fn execute_content_action_returns_text_for_matching_kind() {
    let (service, _, _) = service();
    let key = StorageKey::new("books/book.txt").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("book", key, Bytes::from_static(b"Hello book"))
                .with_kind(ResourceKind::try_new("core:text").unwrap()),
        ),
    )
    .unwrap();

    let output = block_on(service.actions().execute_resource_action(
        &resource.id(),
        ExecuteResourceAction::new("test.text.extract"),
    ))
    .unwrap()
    .unwrap();

    assert_eq!(
        &output.output().view,
        &PluginView::Text(TextView {
            text: "Hello book".to_string()
        })
    );
}

#[test]
fn execute_write_action_replaces_resource_content() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/note.md").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("note.md", key.clone(), Bytes::from_static(b"# Old"))
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();

    let output = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new("azvs.markdown.edit")
                .with_input(json!({"markdown": "# New\n\nUpdated."})),
        ),
    )
    .unwrap()
    .unwrap();

    assert_eq!(output.action().as_str(), "azvs.markdown.edit");
    let updated = repository.find_sync(&resource.id()).unwrap();
    let content = updated.content().unwrap();
    assert!(blob_storage.contains(&key));
    assert_eq!(block_on(service.storage_key(&updated)).unwrap(), key);
    assert!(!blob_storage.contains_fragment(".asset-hub/"));
    assert!(!blob_storage.contains_fragment(".action-replacements/"));
    assert!(!blob_storage.contains_fragment(".action-backups/"));
    assert_eq!(
        blob_storage
            .get_sync(&block_on(service.storage_key(&updated)).unwrap())
            .unwrap(),
        Bytes::from_static(b"# New\n\nUpdated.")
    );
    assert_eq!(content.size(), 15);
    assert_eq!(content.mime_type(), Some("text/markdown"));
    assert_eq!(content.checksum().unwrap().kind(), ChecksumKind::Sha256);
    assert_eq!(
        content.checksum().unwrap().value(),
        hex_sha256(b"# New\n\nUpdated.")
    );
}

#[test]
fn execute_action_rejects_a_stale_caller_snapshot() {
    let (service, _, blob_storage) = service();
    let key = StorageKey::new("docs/stale-note.md").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "stale-note.md",
                key.clone(),
                Bytes::from_static(b"# Current"),
            )
            .with_kind(ResourceKind::try_new("core:text").unwrap())
            .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let stale = resource.revision() + 1;

    let error = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new("azvs.markdown.edit")
                .with_input(json!({"markdown": "# Stale"}))
                .with_expected_revision(stale),
        ),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(
        blob_storage.get_sync(&key).unwrap(),
        Bytes::from_static(b"# Current")
    );
}

#[test]
fn streaming_text_replacement_updates_content_and_revision() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/streamed.md").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("streamed.md", key.clone(), Bytes::from_static(b"# Old"))
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let replacement = Bytes::from_static(b"# Streamed\n\nUpdated.");
    let checksum = Checksum::sha256(hex_sha256(&replacement)).unwrap();
    let command = ReplaceResourceContent::new(
        replacement.len() as u64,
        checksum.clone(),
        resource.revision(),
    )
    .with_mime_type("text/markdown");

    let updated = block_on(service.content().replace_text_content_snapshot(
        repository.locate_sync(resource.clone()),
        command,
        Box::pin(futures_util::stream::once({
            let replacement = replacement.clone();
            async move { Ok(replacement) }
        })),
    ))
    .unwrap();

    assert_eq!(updated.revision(), resource.revision() + 1);
    assert_eq!(updated.content().unwrap().checksum(), Some(&checksum));
    assert_eq!(blob_storage.get_sync(&key), Some(replacement));
    assert_eq!(
        repository.find_sync(&resource.id()).unwrap().revision(),
        updated.revision()
    );
}

#[tokio::test]
async fn streaming_text_replacement_serializes_with_rename_on_the_same_blob() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/concurrent.md").unwrap();
    let renamed_key = StorageKey::new("docs/renamed.md").unwrap();
    let resource = service
        .upload_resource_for_test(
            stream_upload_command(
                "concurrent.md",
                key.clone(),
                Bytes::from_static(b"# Original"),
            )
            .with_kind(ResourceKind::try_new("core:text").unwrap())
            .with_mime_type("text/markdown"),
        )
        .await
        .unwrap();
    let replacement = Bytes::from_static(b"# Replacement");
    let command = ReplaceResourceContent::new(
        replacement.len() as u64,
        Checksum::sha256(hex_sha256(&replacement)).unwrap(),
        resource.revision(),
    );
    let (save_started, release_save) = repository.pause_next_save();

    let replacement_service = service.clone();
    let replacement_snapshot = repository.locate_sync(resource.clone());
    let replacement_task = tokio::spawn(async move {
        replacement_service
            .content()
            .replace_text_content_snapshot(
                replacement_snapshot,
                command,
                Box::pin(futures_util::stream::once(async move { Ok(replacement) })),
            )
            .await
    });
    save_started
        .await
        .expect("replacement should reach its conditional Resource save");

    let rename_service = service.clone();
    let rename_snapshot = repository.locate_sync(resource.clone());
    let rename_task = tokio::spawn(async move {
        rename_service
            .commands()
            .update_resource_snapshot(
                rename_snapshot,
                UpdateResource::new().with_name("renamed.md"),
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !rename_task.is_finished(),
        "rename must wait while replacement owns the original Blob key"
    );

    release_save.send(()).unwrap();
    let updated = replacement_task.await.unwrap().unwrap();
    let rename_error = rename_task.await.unwrap().unwrap_err();

    assert!(matches!(rename_error, CoreError::Conflict { .. }));
    assert_eq!(
        blob_storage.get_sync(&key),
        Some(Bytes::from_static(b"# Replacement"))
    );
    assert!(!blob_storage.contains(&renamed_key));
    assert_eq!(
        repository.find_sync(&resource.id()).unwrap().revision(),
        updated.revision()
    );
}

#[test]
fn streaming_text_replacement_restores_the_original_blob_when_cas_fails() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/rollback.md").unwrap();
    let original = Bytes::from_static(b"# Original");
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("rollback.md", key.clone(), original.clone())
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let replacement = Bytes::from_static(b"# Replacement");
    let command = ReplaceResourceContent::new(
        replacement.len() as u64,
        Checksum::sha256(hex_sha256(&replacement)).unwrap(),
        resource.revision(),
    );
    repository.fail_next_conditional_save();

    let error = block_on(service.content().replace_text_content_snapshot(
        repository.locate_sync(resource.clone()),
        command,
        Box::pin(futures_util::stream::once(async move { Ok(replacement) })),
    ))
    .unwrap_err();

    assert!(matches!(error, CoreError::Repository { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(original));
    assert_eq!(
        repository.find_sync(&resource.id()).unwrap().revision(),
        resource.revision()
    );
}

#[test]
fn startup_recovery_rolls_back_a_published_replacement_without_a_resource_commit() {
    let (service, repository, blob_storage) = service();
    let target = StorageKey::new("docs/interrupted.md").unwrap();
    let original = Bytes::from_static(b"# Original");
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("interrupted.md", target.clone(), original.clone())
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let replacement_bytes = Bytes::from_static(b"# Interrupted replacement");
    let replacement_content = crate::domain::ResourceContent::verified(
        replacement_bytes.len() as u64,
        Checksum::sha256(hex_sha256(&replacement_bytes)).unwrap(),
    )
    .with_mime_type("text/markdown")
    .build()
    .unwrap();
    let id = ResourceContentReplacementId::new();
    let staged = StorageKey::new(format!(".asset-hub/uploads/replacement-{id}")).unwrap();
    let backup = StorageKey::new(format!(".asset-hub/content-backups/{id}")).unwrap();
    let pending = ResourceContentReplacement::rehydrate(
        id,
        resource.id(),
        resource.revision(),
        target.clone(),
        staged.clone(),
        backup.clone(),
        replacement_content,
    )
    .unwrap();
    block_on(service.content_replacements.save(&pending)).unwrap();
    block_on(blob_storage.put(&staged, replacement_bytes.clone())).unwrap();
    block_on(blob_storage.move_if_absent(&target, &backup)).unwrap();
    block_on(blob_storage.put(&target, replacement_bytes)).unwrap();

    assert_eq!(block_on(service.resume_content_replacements()).unwrap(), 1);

    assert_eq!(blob_storage.get_sync(&target), Some(original));
    assert!(!blob_storage.contains(&backup));
    assert!(!blob_storage.contains(&staged));
    assert_eq!(repository.find_sync(&resource.id()).unwrap(), resource);
    assert!(
        block_on(service.content_replacements.list_pending())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn startup_recovery_keeps_a_committed_replacement_and_cleans_internal_blobs() {
    let (service, repository, blob_storage) = service();
    let target = StorageKey::new("docs/committed.md").unwrap();
    let original = Bytes::from_static(b"# Original");
    let mut resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("committed.md", target.clone(), original)
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let expected_revision = resource.revision();
    let replacement_bytes = Bytes::from_static(b"# Committed replacement");
    let replacement_content = crate::domain::ResourceContent::verified(
        replacement_bytes.len() as u64,
        Checksum::sha256(hex_sha256(&replacement_bytes)).unwrap(),
    )
    .with_mime_type("text/markdown")
    .build()
    .unwrap();
    let id = ResourceContentReplacementId::new();
    let staged = StorageKey::new(format!(".asset-hub/uploads/replacement-{id}")).unwrap();
    let backup = StorageKey::new(format!(".asset-hub/content-backups/{id}")).unwrap();
    let pending = ResourceContentReplacement::rehydrate(
        id,
        resource.id(),
        expected_revision,
        target.clone(),
        staged.clone(),
        backup.clone(),
        replacement_content.clone(),
    )
    .unwrap();
    block_on(service.content_replacements.save(&pending)).unwrap();
    block_on(blob_storage.put(&staged, replacement_bytes.clone())).unwrap();
    block_on(blob_storage.move_if_absent(&target, &backup)).unwrap();
    block_on(blob_storage.put(&target, replacement_bytes.clone())).unwrap();
    resource.attach_content(replacement_content).unwrap();
    block_on(repository.save(&resource)).unwrap();

    assert_eq!(block_on(service.resume_content_replacements()).unwrap(), 1);

    assert_eq!(blob_storage.get_sync(&target), Some(replacement_bytes));
    assert!(!blob_storage.contains(&backup));
    assert!(!blob_storage.contains(&staged));
    assert!(
        block_on(service.content_replacements.list_pending())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn streaming_text_replacement_rejects_bad_checksums_and_oversized_content() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/validated.md").unwrap();
    let original = Bytes::from_static(b"# Original");
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("validated.md", key.clone(), original.clone())
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let replacement = Bytes::from_static(b"changed");
    let bad_checksum = Checksum::sha256("0".repeat(64)).unwrap();
    let error = block_on(service.content().replace_text_content_snapshot(
        repository.locate_sync(resource.clone()),
        ReplaceResourceContent::new(replacement.len() as u64, bad_checksum, resource.revision()),
        Box::pin(futures_util::stream::once(async move { Ok(replacement) })),
    ))
    .unwrap_err();
    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(original.clone()));

    let max = test_resource_content_edit_policy().max_text_bytes();
    let error = block_on(service.content().replace_text_content_snapshot(
        repository.locate_sync(resource.clone()),
        ReplaceResourceContent::new(
            max + 1,
            Checksum::sha256("0".repeat(64)).unwrap(),
            resource.revision(),
        ),
        Box::pin(futures_util::stream::empty()),
    ))
    .unwrap_err();
    assert!(matches!(error, CoreError::LimitExceeded { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(original));
}

#[test]
fn text_edit_capability_is_hidden_above_the_edit_policy() {
    let (service, _, _) = service();
    let resource = Resource::builder("large.md")
        .with_kind(ResourceKind::try_new("core:text").unwrap())
        .with_content(
            crate::domain::ResourceContent::verified(
                test_resource_content_edit_policy().max_text_bytes() + 1,
                Checksum::sha256("0".repeat(64)).unwrap(),
            )
            .with_mime_type("text/markdown")
            .build()
            .unwrap(),
        )
        .build()
        .unwrap();

    let actions = service.describe_resource_actions(&resource).unwrap();
    assert!(actions.available_actions().iter().all(|action| {
        !action
            .provides()
            .is_some_and(|capability| capability.as_str() == "text_edit")
    }));
}

#[test]
fn member_cannot_replace_text_outside_the_workspace() {
    let (service, _, _) = service();
    let root = block_on(service.directory_service().root()).unwrap();
    let workspace = block_on(service.directory_service().create(&root, "workspace")).unwrap();
    let outside = block_on(service.directory_service().create(&root, "outside")).unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "outside.md",
                StorageKey::new("outside/outside.md").unwrap(),
                Bytes::from_static(b"outside"),
            )
            .with_kind(ResourceKind::try_new("core:text").unwrap())
            .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    assert_eq!(resource.directory_id(), outside.id());
    let user = User::new("member", "hash", UserRole::Member, workspace.id()).unwrap();
    let context = AccessContext::member(user.id());
    let authorization = crate::service::AuthorizationService::new(
        Arc::new(SingleUserRepository(user)),
        service.directory_service().clone(),
    );
    let replacement = Bytes::from_static(b"denied");
    let command = ReplaceResourceContent::new(
        replacement.len() as u64,
        Checksum::sha256(hex_sha256(&replacement)).unwrap(),
        resource.revision(),
    );

    let error = block_on(
        service
            .secured(&authorization, &context)
            .replace_resource_content(
                &resource.id(),
                command,
                Box::pin(futures_util::stream::once(async move { Ok(replacement) })),
            ),
    )
    .unwrap_err();
    assert!(matches!(error, CoreError::Forbidden { .. }));
}

#[test]
fn write_action_cleanup_failure_keeps_a_recoverable_intent() {
    let (service, _repository, blob_storage) = service();
    let key = StorageKey::new("docs/note.md").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("note.md", key, Bytes::from_static(b"# Old"))
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    blob_storage.fail_next_delete();

    let error = block_on(service.actions().execute_resource_action(
        &resource.id(),
        ExecuteResourceAction::new("azvs.markdown.edit").with_input(json!({"markdown": "# New"})),
    ))
    .unwrap_err();

    assert!(matches!(error, CoreError::Storage { .. }));
    assert!(blob_storage.contains_fragment(".asset-hub/content-backups/"));
    assert_eq!(
        block_on(service.content_replacements.list_pending())
            .unwrap()
            .len(),
        1
    );

    assert_eq!(block_on(service.resume_content_replacements()).unwrap(), 1);
    assert!(!blob_storage.contains_fragment(".asset-hub/content-backups/"));
    assert!(!blob_storage.contains_fragment(".asset-hub/uploads/replacement-"));
    assert!(
        block_on(service.content_replacements.list_pending())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn describe_resource_actions_uses_declared_content_matchers() {
    let (service, _, _) = service();
    let pdf = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "book",
                StorageKey::new("books/book.pdf").unwrap(),
                Bytes::from_static(b"%PDF-1.4"),
            )
            .with_kind(ResourceKind::try_new("core:text").unwrap())
            .with_mime_type("application/pdf"),
        ),
    )
    .unwrap();
    let text = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "book",
                StorageKey::new("books/book.txt").unwrap(),
                Bytes::from_static(b"hello"),
            )
            .with_kind(ResourceKind::try_new("core:text").unwrap())
            .with_mime_type("text/plain"),
        ),
    )
    .unwrap();

    let pdf_actions = service.actions().describe_resource_actions(&pdf).unwrap();
    let text_actions = service.actions().describe_resource_actions(&text).unwrap();
    let has_action = |actions: &ResourceActions, id: &str| {
        actions
            .available_actions()
            .iter()
            .any(|action| action.id().as_str() == id)
    };

    assert!(has_action(&pdf_actions, "test.text.extract"));
    assert!(has_action(&pdf_actions, "test.text.thumbnail"));
    assert!(!has_action(&pdf_actions, "core.resource.thumbnail"));
    assert!(!has_action(&pdf_actions, "azvs.markdown.read"));
    assert!(has_action(&text_actions, "test.text.extract"));
    assert!(!has_action(&text_actions, "test.text.thumbnail"));
    assert!(has_action(&text_actions, "core.resource.thumbnail"));
    assert!(!has_action(&text_actions, "azvs.markdown.read"));
}

#[test]
fn soft_delete_resource_moves_blob_to_trash_and_hides_content_read() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key.clone(),
        data.clone(),
    )))
    .unwrap();
    let trash_key = StorageKey::new(format!(".asset-hub/trash/{}", resource.id())).unwrap();

    let deleted = block_on(service.commands().soft_delete_resource(&resource.id()))
        .unwrap()
        .unwrap();
    let content = block_on(service.content().get_resource_content(&resource.id())).unwrap();

    assert!(deleted.is_deleted());
    assert!(repository.find_sync(&resource.id()).unwrap().is_deleted());
    assert!(!blob_storage.contains(&key));
    assert_eq!(blob_storage.get_sync(&trash_key), Some(data));
    assert!(content.is_none());
}

#[test]
fn restoring_soft_deleted_resource_moves_blob_back_from_trash() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/restored.png").unwrap();
    let data = Bytes::from_static(b"restored bytes");
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "restored",
        key.clone(),
        data.clone(),
    )))
    .unwrap();
    let trash_key = StorageKey::new(format!(".asset-hub/trash/{}", resource.id())).unwrap();
    let deleted = block_on(service.commands().soft_delete_resource(&resource.id()))
        .unwrap()
        .unwrap();

    let restored = block_on(service.commands().update_resource_snapshot(
        repository.locate_sync(deleted),
        UpdateResource::new().with_restore(true),
    ))
    .unwrap();

    assert!(!restored.is_deleted());
    assert_eq!(blob_storage.get_sync(&key), Some(data));
    assert!(!blob_storage.contains(&trash_key));
}

#[test]
fn soft_delete_rolls_blob_back_when_resource_snapshot_is_stale() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/concurrent-delete.png").unwrap();
    let data = Bytes::from_static(b"still active");
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "concurrent-delete",
        key.clone(),
        data.clone(),
    )))
    .unwrap();
    let stale = resource.clone();
    let trash_key = StorageKey::new(format!(".asset-hub/trash/{}", resource.id())).unwrap();
    let mut concurrent = resource;
    concurrent
        .replace_tags(vec!["concurrent".to_owned()])
        .unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(
        service
            .commands()
            .soft_delete_resource_snapshot(repository.locate_sync(stale)),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(data));
    assert!(!blob_storage.contains(&trash_key));
    assert!(!repository.find_sync(&concurrent.id()).unwrap().is_deleted());
}

#[test]
fn remove_resource_deletes_blob_and_repository_record() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key.clone(),
        Bytes::from_static(b"image bytes"),
    )))
    .unwrap();

    assert!(block_on(service.commands().remove_resource(&resource.id())).unwrap());
    assert!(repository.find_sync(&resource.id()).is_none());
    assert!(!blob_storage.contains(&key));
    assert!(!block_on(service.commands().remove_resource(&resource.id())).unwrap());
}

#[test]
fn remove_resource_rejects_a_stale_authorized_snapshot_without_deleting_content() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/concurrent.png").unwrap();
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key.clone(),
        Bytes::from_static(b"image bytes"),
    )))
    .unwrap();
    let stale = resource.clone();
    let mut concurrent = resource;
    concurrent.rename("moved by another request").unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(
        service
            .commands()
            .remove_resource_snapshot(repository.locate_sync(stale)),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert!(repository.find_sync(&concurrent.id()).is_some());
    assert!(blob_storage.contains(&key));
}
