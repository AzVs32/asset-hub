use super::action::resolved_content_delivery;
use super::content::hex_sha256;
use super::*;
use crate::domain::{
    AccessContext, ActionAccess, Checksum, ContentVerificationStatus, DefinitionOrigin, Directory,
    DirectoryActionDefinition, DirectoryId, DirectoryKindDefinition, DirectoryPath,
    ResourceActionDefinition, ResourceActionPolicy, ResourceContentEditPolicy,
    ResourceContentMatcher, ResourceContentReplacement, ResourceContentReplacementId, ResourceId,
    UploadId, UploadSession, UploadStatus, User, UserId, UserRole,
};
use crate::port::{
    BlobByteStream, DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRegistry,
    DirectoryActionRequest, DirectoryIndex, DirectoryKindRegistry, DirectoryLocation,
    DirectoryQuery, DirectoryRepository, DirectoryStorage, ListResources, LocatedDirectory,
    LocatedResource, ResourceActionOutput, ResourceActionRequest,
    ResourceContentReplacementRepository, ResourceKindRegistry, ResourcePage, ScannedStorageEntry,
    StagedBlob, StoragePrefix, UploadSessionRepository, UserRepository,
};
use asset_plugin_api::protocol::directory::{
    DirectoryActionEffect, PluginDirectoryActionOutput, UpdateDirectoryEffect,
};
use asset_plugin_api::protocol::{
    PluginReplacementEncoding, PluginResourceActionEffect, PluginResourceActionOutput, PluginView,
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
        session.synchronize_offset(offset)?;
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
        session.mark_finalizing()?;
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
        session.set_actual_checksum(checksum.clone())?;
        Ok(())
    }

    async fn mark_completed(&self, id: &UploadId) -> Result<(), CoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| CoreError::not_found("upload", id.to_string()))?;
        session.mark_completed()?;
        Ok(())
    }

    async fn mark_failed(&self, id: &UploadId, failure: &str) -> Result<(), CoreError> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| CoreError::not_found("upload", id.to_string()))?;
        session.mark_failed(failure)?;
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
impl DirectoryRepository for InMemoryResourceRepository {
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
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let current = self
            .directories
            .lock()
            .unwrap()
            .get(&directory.id())
            .cloned();
        if !current.is_some_and(|(current, _)| current.revision() == expected_revision) {
            return Ok(false);
        }
        self.insert(directory).await?;
        Ok(true)
    }

    async fn remove_if_empty(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let mut directories = self.directories.lock().unwrap();
        if !directories
            .get(id)
            .is_some_and(|(directory, _)| directory.revision() == expected_revision)
        {
            return Ok(false);
        }
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
            definitions: vec![DirectoryKindDefinition::new(
                crate::domain::DirectoryKind::default(),
                "Directory",
                DefinitionOrigin::builtin_static("test"),
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
                let mut output = PluginResourceActionOutput::new(PluginView::Text(TextView {
                    text: "saved".to_string(),
                }));
                output
                    .effects
                    .push(PluginResourceActionEffect::ReplaceContent(
                        ReplaceContentEffect {
                            encoding: PluginReplacementEncoding::Base64,
                            data: STANDARD.encode(markdown),
                            mime_type: Some("text/markdown".to_string()),
                        },
                    ));

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
            PluginResourceActionOutput::new(view),
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
        let mut output = PluginDirectoryActionOutput::new(PluginView::Text(TextView {
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
        ResourceKindDefinition::new(
            ResourceKind::default(),
            "Resource",
            true,
            DefinitionOrigin::builtin_static("test"),
        ),
        ResourceKindDefinition::new(
            ResourceKind::try_new("doc:markdown").unwrap(),
            "Markdown",
            false,
            DefinitionOrigin::builtin_static("test"),
        ),
        ResourceKindDefinition::new(
            ResourceKind::try_new("core:image").unwrap(),
            "Image",
            true,
            DefinitionOrigin::builtin_static("test"),
        ),
        ResourceKindDefinition::new(
            ResourceKind::try_new("asset:binary").unwrap(),
            "Binary",
            true,
            DefinitionOrigin::builtin_static("test"),
        ),
        ResourceKindDefinition::new(
            ResourceKind::try_new("core:text").unwrap(),
            "Text",
            true,
            DefinitionOrigin::builtin_static("test"),
        ),
        ResourceKindDefinition::new(
            ResourceKind::try_new("azvs:markdown").unwrap(),
            "Markdown",
            true,
            DefinitionOrigin::builtin_static("test"),
        )
        .with_parent(Some(ResourceKind::try_new("core:text").unwrap()))
        .with_detect(
            ResourceContentMatcher::new()
                .with_mime_types(["text/markdown", "text/x-markdown"])
                .with_extensions([".md", ".markdown"]),
        ),
        ResourceKindDefinition::new(
            ResourceKind::try_new("core:video").unwrap(),
            "Video",
            true,
            DefinitionOrigin::builtin_static("test"),
        ),
    ]));
    let action_registry = Arc::new(InMemoryResourceActionRegistry {
        actions: vec![
            ResourceActionDefinition::new_static("test.text.extract", "Extract text")
                .with_kinds(["doc:markdown", "core:text"])
                .with_requirements(content_requirements())
                .with_output(output_contract(["text"])),
            ResourceActionDefinition::new_static("resource.inspect", "Inspect resource")
                .with_kinds(["doc:markdown"])
                .with_output(output_contract(["json"])),
            ResourceActionDefinition::new_static("doc.markdown.thumbnail", "Markdown thumbnail")
                .with_static_provides(Some("thumbnail"))
                .with_kinds(["doc:markdown"])
                .with_requirements(content_requirements())
                .with_output(output_contract(["media"])),
            ResourceActionDefinition::new_static("test.text.thumbnail", "Text thumbnail")
                .with_static_provides(Some("thumbnail"))
                .with_kinds(["core:text"])
                .with_content_matcher(
                    ResourceContentMatcher::new().with_mime_types(["application/pdf"]),
                )
                .with_output(output_contract(["media"])),
            ResourceActionDefinition::new_static("core.resource.thumbnail", "Thumbnail")
                .with_static_provides(Some("thumbnail"))
                .with_output(output_contract(["media"])),
            ResourceActionDefinition::new_static("azvs.markdown.read", "Read Markdown")
                .with_static_provides(Some("text_read"))
                .with_kinds(["core:text"])
                .with_requirements(content_requirements())
                .with_output(output_contract(["plugin_frame"]))
                .with_content_matcher(
                    ResourceContentMatcher::new()
                        .with_mime_types(["text/markdown", "text/x-markdown"])
                        .with_extensions([".md", ".markdown"]),
                ),
            ResourceActionDefinition::new_static("azvs.markdown.edit", "Edit Markdown")
                .with_static_provides(Some("text_edit"))
                .with_kinds(["core:text"])
                .with_requirements(content_requirements())
                .with_access(ActionAccess::Write)
                .with_output(effect_output_contract(["text"], ["replace_content"]))
                .with_content_matcher(
                    ResourceContentMatcher::new()
                        .with_mime_types(["text/markdown", "text/x-markdown"])
                        .with_extensions([".md", ".markdown"]),
                ),
        ],
    });
    let repository = Arc::new(InMemoryResourceRepository::default());
    let blob_storage = Arc::new(InMemoryBlobStorage::default());
    let directories = DirectoryService::new(
        repository.clone(),
        repository.clone(),
        blob_storage.clone(),
        Arc::new(InMemoryDirectoryKindRegistry::default()),
    )
    .with_actions(
        Arc::new(InMemoryDirectoryActionRegistry {
            actions: vec![
                DirectoryActionDefinition::new_static("test.directory.move", "Move directory")
                    .with_kinds(["core:directory"])
                    .with_access(ActionAccess::Write)
                    .with_output(effect_output_contract(["text"], ["update"])),
            ],
        }),
        Arc::new(StaticDirectoryActionExecutor),
    );
    let service = ResourceService::new(
        ResourceServicePorts::new(
            repository.clone(),
            repository.clone(),
            blob_storage.clone(),
            blob_storage.clone(),
            kind_registry,
            Arc::new(InMemoryUploadSessionRepository::default()),
            Arc::new(InMemoryContentReplacementRepository::default()),
        )
        .with_actions(action_registry, Arc::new(StaticResourceActionExecutor)),
        directories,
        Arc::new(test_resource_action_policy()),
        Arc::new(test_resource_content_edit_policy()),
    );

    (service, repository, blob_storage)
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

fn output_contract<const N: usize>(views: [&str; N]) -> crate::domain::ActionOutputContract {
    crate::domain::ActionOutputContract {
        views: views.into_iter().map(str::to_string).collect(),
        effects: Vec::new(),
    }
}

fn effect_output_contract<const V: usize, const E: usize>(
    views: [&str; V],
    effects: [&str; E],
) -> crate::domain::ActionOutputContract {
    crate::domain::ActionOutputContract {
        views: views.into_iter().map(str::to_string).collect(),
        effects: effects.into_iter().map(str::to_string).collect(),
    }
}

struct TestUpload {
    name: String,
    kind: Option<ResourceKind>,
    directory: DirectoryPath,
    content: BlobByteStream,
    mime_type: Option<String>,
}

impl TestUpload {
    fn new(name: impl Into<String>, content: BlobByteStream) -> Self {
        Self {
            name: name.into(),
            kind: None,
            directory: DirectoryPath::root(),
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
            .with_directory(directory);
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

mod action_tests;
mod command_tests;
mod content_tests;
mod reconciliation_tests;
mod secured_tests;
mod upload_tests;
