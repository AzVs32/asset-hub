use super::*;
use crate::port::{BlobWriteResult, ResourceKindDefinition, ResourceKindRegistry};
use asset_plugin_api::{
    MediaView, PluginActionEffect, PluginActionOutput, PluginMediaEncoding,
    PluginReplacementEncoding, PluginView, ReplaceContentEffect, ResourceAction,
    ResourceActionAccess, ResourceActionDefinition, ResourceContentMatcher, TextView,
};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures_util::StreamExt;
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

#[derive(Default)]
struct InMemoryResourceRepository {
    resources: Mutex<HashMap<ResourceId, Resource>>,
    fail_next_save: Mutex<bool>,
}

impl InMemoryResourceRepository {
    fn fail_next_save(&self) {
        *self.fail_next_save.lock().unwrap() = true;
    }

    fn find_sync(&self, id: &ResourceId) -> Option<Resource> {
        self.resources.lock().unwrap().get(id).cloned()
    }

    fn is_empty(&self) -> bool {
        self.resources.lock().unwrap().is_empty()
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

        self.resources
            .lock()
            .unwrap()
            .insert(resource.id(), resource.clone());

        Ok(())
    }

    async fn save_if_unchanged(
        &self,
        resource: &Resource,
        expected_updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CoreError> {
        let mut resources = self.resources.lock().unwrap();
        let Some(current) = resources.get(&resource.id()) else {
            return Ok(false);
        };
        if current.updated_at() != expected_updated_at {
            return Ok(false);
        }
        resources.insert(resource.id(), resource.clone());
        Ok(true)
    }

    async fn remove_if_unchanged(
        &self,
        id: &ResourceId,
        expected_updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CoreError> {
        let mut resources = self.resources.lock().unwrap();
        let Some(current) = resources.get(id) else {
            return Ok(false);
        };
        if current.updated_at() != expected_updated_at {
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
    async fn find_by_path(
        &self,
        directory: &ResourceDirectory,
        name: &str,
    ) -> Result<Option<Resource>, CoreError> {
        Ok(self
            .resources
            .lock()
            .unwrap()
            .values()
            .find(|resource| {
                !resource.is_deleted()
                    && resource.directory() == directory
                    && resource.name() == name
            })
            .cloned())
    }

    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError> {
        let mut resources = self
            .resources
            .lock()
            .unwrap()
            .values()
            .filter(|resource| query.include_deleted() || !resource.is_deleted())
            .filter(|resource| {
                query
                    .kind()
                    .is_none_or(|kind| resource.kind().as_str() == kind.as_str())
            })
            .filter(|resource| {
                query
                    .tag()
                    .is_none_or(|tag| resource.tags().iter().any(|value| value.as_str() == tag))
            })
            .filter(|resource| query.q().is_none_or(|q| resource.name().contains(q)))
            .filter(|resource| {
                query
                    .directory()
                    .is_none_or(|directory| resource.directory() == directory)
            })
            .cloned()
            .collect::<Vec<_>>();
        resources.sort_by_key(|resource| std::cmp::Reverse(resource.updated_at()));

        let total = resources.len() as u64;
        let items = resources
            .into_iter()
            .skip(query.offset() as usize)
            .take(query.limit() as usize)
            .collect();

        Ok(ResourcePage {
            items,
            total,
            limit: query.limit(),
            offset: query.offset(),
        })
    }

    async fn list_directories(
        &self,
        parent: &ResourceDirectory,
    ) -> Result<Vec<ResourceDirectory>, CoreError> {
        let mut directories = self
            .resources
            .lock()
            .unwrap()
            .values()
            .filter_map(|resource| child_directory(resource.directory(), parent))
            .collect::<Vec<_>>();
        directories.sort_by(|left, right| left.path().cmp(right.path()));
        directories.dedup_by(|left, right| left.path() == right.path());
        Ok(directories)
    }
}

fn child_directory(
    directory: &ResourceDirectory,
    parent: &ResourceDirectory,
) -> Option<ResourceDirectory> {
    if directory.is_root() {
        return None;
    }
    let directory = directory.path();
    let remainder = if parent.is_root() {
        directory
    } else {
        directory.strip_prefix(parent.path())?.strip_prefix('/')?
    };
    parent.child(remainder.split('/').next()?).ok()
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

#[derive(Default)]
struct InMemoryBlobStorage {
    objects: Mutex<HashMap<StorageKey, Bytes>>,
    fail_next_delete: Mutex<bool>,
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
}

#[async_trait::async_trait]
impl BlobStorage for InMemoryBlobStorage {
    async fn health_check(&self) -> Result<(), CoreError> {
        Ok(())
    }

    async fn put(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
        self.objects.lock().unwrap().insert(key.clone(), data);
        Ok(())
    }

    async fn put_stream_if_absent(
        &self,
        key: &StorageKey,
        mut data: BlobByteStream,
    ) -> Result<BlobWriteResult, CoreError> {
        let mut bytes = Vec::new();

        while let Some(chunk) = data.next().await {
            bytes.extend_from_slice(&chunk?);
        }

        let bytes_written = bytes.len() as u64;
        let mut objects = self.objects.lock().unwrap();
        if objects.contains_key(key) {
            return Err(CoreError::conflict(format!(
                "storage key `{key}` already exists"
            )));
        }

        objects.insert(key.clone(), Bytes::from(bytes));

        Ok(BlobWriteResult::new(bytes_written))
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
        Ok(())
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), CoreError> {
        if std::mem::take(&mut *self.fail_next_delete.lock().unwrap()) {
            return Err(CoreError::storage("delete", TestError("delete failed")));
        }
        self.objects.lock().unwrap().remove(key);
        Ok(())
    }
}

#[async_trait::async_trait]
impl StorageScanner for InMemoryBlobStorage {
    async fn scan(
        &self,
        prefix: &StoragePrefix,
        include_sha256: bool,
        _max_entries: usize,
    ) -> Result<Vec<crate::port::ScannedBlob>, CoreError> {
        if prefix.as_str() == crate::port::RESERVED_BLOB_STORAGE_PREFIX
            || prefix
                .as_str()
                .starts_with(&format!("{}/", crate::port::RESERVED_BLOB_STORAGE_PREFIX))
        {
            return Ok(Vec::new());
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
                sha256: include_sha256.then(|| hex_sha256(content)),
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.key.as_str().cmp(right.key.as_str()));
        Ok(files)
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
            ResourceAction::READ => PluginView::Text(TextView {
                text: String::from_utf8(
                    request
                        .content()
                        .map(|content| content.to_vec())
                        .unwrap_or_default(),
                )
                .unwrap(),
            }),
            ResourceAction::PREVIEW | ResourceAction::THUMBNAIL => {
                let content = request.content().cloned().unwrap_or_default();
                PluginView::Media(MediaView {
                    mime_type: request
                        .resource()
                        .content()
                        .and_then(|content| content.mime_type())
                        .unwrap_or("application/octet-stream")
                        .to_string(),
                    title: Some(request.resource().name().to_string()),
                    encoding: PluginMediaEncoding::Base64,
                    data: STANDARD.encode(content),
                })
            }
            "azvs.markdown.update" => {
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

struct NoopWaker;

impl Wake for NoopWaker {
    fn wake(self: Arc<Self>) {}
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWaker));
    let mut context = Context::from_waker(&waker);
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
        ResourceKindDefinition::new(ResourceKind::default(), "Unknown", true),
        ResourceKindDefinition::new(ResourceKind::try_new("core:file").unwrap(), "File", true),
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
        ResourceKindDefinition::new(
            ResourceKind::try_new("core:document").unwrap(),
            "Document",
            true,
        ),
        ResourceKindDefinition::new(
            ResourceKind::try_new("azvs:markdown").unwrap(),
            "Markdown Document",
            true,
        )
        .with_parent(Some(ResourceKind::try_new("core:document").unwrap()))
        .with_detect(
            ResourceContentMatcher::new()
                .with_mime_types(["text/markdown", "text/x-markdown"])
                .with_extensions([".md", ".markdown"]),
        ),
        ResourceKindDefinition::new(ResourceKind::try_new("core:video").unwrap(), "Video", true),
    ]));
    let action_registry = Arc::new(InMemoryResourceActionRegistry {
        actions: vec![
            ResourceActionDefinition::new(ResourceAction::READ, "Read")
                .with_kinds(["doc:markdown", "core:document"])
                .with_handler("read_document")
                .with_requirements(content_requirements())
                .with_output(output_contract(["text"])),
            ResourceActionDefinition::new("resource.inspect", "Inspect resource")
                .with_kinds(["doc:markdown"])
                .with_handler("inspect_resource")
                .with_output(output_contract(["json"])),
            ResourceActionDefinition::new(ResourceAction::PREVIEW, "Preview")
                .with_kinds(["core:image", "core:document", "core:video"])
                .with_handler("preview_document")
                .with_requirements(content_requirements())
                .with_output(output_contract(["media"])),
            ResourceActionDefinition::new(ResourceAction::THUMBNAIL, "Thumbnail")
                .with_kinds(["core:image"])
                .with_handler("thumbnail_image")
                .with_requirements(content_requirements())
                .with_output(output_contract(["media"])),
            ResourceActionDefinition::new("azvs.markdown.render", "Read Markdown")
                .with_kinds(["core:document"])
                .with_handler("render_markdown")
                .with_requirements(content_requirements())
                .with_output(output_contract(["plugin_frame"]))
                .with_content_matcher(
                    ResourceContentMatcher::new()
                        .with_mime_types(["text/markdown", "text/x-markdown"])
                        .with_extensions([".md", ".markdown"]),
                ),
            ResourceActionDefinition::new("azvs.markdown.update", "Edit Markdown")
                .with_kinds(["core:document"])
                .with_handler("update_markdown")
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
            blob_storage.clone(),
            kind_registry,
        )
        .with_actions(action_registry, Arc::new(StaticResourceActionExecutor)),
        Arc::new(test_plugin_execution_policy()),
    );

    (service, repository, blob_storage)
}

fn content_requirements() -> asset_plugin_api::ResourceActionRequirements {
    asset_plugin_api::ResourceActionRequirements {
        content: true,
        content_delivery: asset_plugin_api::ResourceActionContentDelivery::Inline,
    }
}

fn test_plugin_execution_policy() -> PluginExecutionPolicy {
    PluginExecutionPolicy::new(
        64 * 1024 * 1024,
        4 * 1024 * 1024,
        4 * 1024 * 1024,
        8 * 1024 * 1024,
        8 * 1024 * 1024,
        8,
        4096,
        20,
    )
    .unwrap()
}

fn output_contract<const N: usize>(
    views: [&str; N],
) -> asset_plugin_api::ResourceActionOutputContract {
    asset_plugin_api::ResourceActionOutputContract {
        view: views.into_iter().map(str::to_string).collect(),
    }
}

fn stream_upload_command(
    _name: impl Into<String>,
    storage_key: StorageKey,
    data: Bytes,
) -> UploadResourceContentStream {
    let (directory, name) = storage_key
        .as_str()
        .rsplit_once('/')
        .map(|(directory, name)| (directory, name))
        .unwrap_or(("", storage_key.as_str()));
    let stream = futures_util::stream::once(async move { Ok(data) });
    UploadResourceContentStream::new(name, Box::pin(stream))
        .with_directory(ResourceDirectory::from_path(directory).unwrap())
}

#[test]
fn action_content_delivery_never_loads_unrequested_content() {
    use asset_plugin_api::{ResourceActionContentDelivery, ResourceActionRequirements};
    let policy = test_plugin_execution_policy();

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
            blob_storage.clone(),
            kind_registry,
        ),
        Arc::new(test_plugin_execution_policy()),
    );

    (service, repository, blob_storage)
}

#[test]
fn create_resource_saves_resource_without_content() {
    let (service, repository, _) = service();

    let resource = block_on(
        service.commands().create_resource(
            CreateResource::new(" Design Doc ")
                .with_kind("doc:markdown")
                .with_description(" Design document ")
                .with_tags(["rust", "asset"]),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();

    assert_eq!(resource.name(), "Design Doc");
    assert!(resource.kind().is("doc:markdown"));
    assert!(resource.content().is_none());
    assert_eq!(saved.description(), Some("Design document"));
    assert_eq!(
        saved
            .tags()
            .iter()
            .map(|tag| tag.as_str())
            .collect::<Vec<_>>(),
        vec!["asset", "rust"]
    );
}

#[test]
fn update_resource_rejects_a_stale_authorized_snapshot() {
    let (service, repository, _) = service();
    let resource = block_on(
        service
            .commands()
            .create_resource(CreateResource::new("original").with_kind("doc:markdown")),
    )
    .unwrap();
    let stale = resource.clone();
    let mut concurrent = resource;
    concurrent.rename("concurrent").unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(
        service
            .commands()
            .update_resource_snapshot(stale, UpdateResource::new().with_name("stale")),
    )
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
    let resource = block_on(
        service
            .commands()
            .create_resource(CreateResource::new("contentless").with_kind("doc:markdown")),
    )
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

    assert_eq!(ids, vec!["resource.inspect"]);
}

#[test]
fn resource_without_content_rejects_direct_content_action_execution() {
    let (service, _, _) = service();
    let resource = block_on(
        service
            .commands()
            .create_resource(CreateResource::new("contentless").with_kind("doc:markdown")),
    )
    .unwrap();

    let error = block_on(service.actions().execute_resource_action(
        &resource.id(),
        ExecuteResourceAction::new(ResourceAction::READ),
    ))
    .unwrap_err();

    assert!(error.to_string().contains("does not support action `read`"));
}

#[test]
fn stream_upload_resource_content_writes_blob_then_saves_resource() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let checksum = Checksum::sha256(hex_sha256(&data)).unwrap();

    let resource = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command("image", key.clone(), data.clone())
                .with_kind("core:image")
                .with_mime_type(" image/png "),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();
    let content = saved.content().unwrap();

    assert_eq!(saved.storage_key(), key);
    assert_eq!(content.size(), data.len() as u64);
    assert_eq!(content.mime_type(), Some("image/png"));
    assert_eq!(content.checksum(), &checksum);
    assert_eq!(blob_storage.get_sync(&key), Some(data));
}

#[test]
fn audit_storage_reports_missing_mismatched_and_orphaned_blobs() {
    let (service, _, blob_storage) = service();
    let missing_key = StorageKey::new("docs/missing.md").unwrap();
    let mismatch_key = StorageKey::new("docs/mismatch.md").unwrap();
    let orphan_key = StorageKey::new("docs/orphan.md").unwrap();
    block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command(
                "missing.md",
                missing_key.clone(),
                Bytes::from_static(b"# Missing"),
            )
            .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command(
                "mismatch.md",
                mismatch_key.clone(),
                Bytes::from_static(b"# Original"),
            )
            .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    blob_storage.objects.lock().unwrap().remove(&missing_key);
    blob_storage
        .objects
        .lock()
        .unwrap()
        .insert(mismatch_key, Bytes::from_static(b"# Changed"));
    blob_storage
        .objects
        .lock()
        .unwrap()
        .insert(orphan_key, Bytes::from_static(b"# Orphan"));

    let result = block_on(
        service
            .content()
            .audit_storage(AuditStorage::new(StoragePrefix::root()).with_sha256(true)),
    )
    .unwrap();

    assert_eq!(result.checked_resources, 2);
    assert_eq!(result.missing, 1);
    assert_eq!(result.orphaned, 1);
    assert!(result.mismatched >= 1);
    assert!(result.issues.iter().any(|issue| {
        issue.kind == AuditStorageIssueKind::MissingBlob && issue.key == "docs/missing.md"
    }));
    assert!(result.issues.iter().any(|issue| {
        issue.kind == AuditStorageIssueKind::ChecksumMismatch && issue.key == "docs/mismatch.md"
    }));
    assert!(result.issues.iter().any(|issue| {
        issue.kind == AuditStorageIssueKind::OrphanBlob && issue.key == "docs/orphan.md"
    }));
}

#[test]
fn audit_storage_ignores_soft_deleted_content_in_internal_trash() {
    let (service, _, _) = service();
    let resource = block_on(service.content().upload_resource_content_stream(
        stream_upload_command(
            "deleted",
            StorageKey::new("docs/deleted.md").unwrap(),
            Bytes::from_static(b"deleted"),
        ),
    ))
    .unwrap();
    block_on(service.commands().soft_delete_resource(&resource.id()))
        .unwrap()
        .unwrap();

    let result = block_on(
        service
            .content()
            .audit_storage(AuditStorage::new(StoragePrefix::root()).with_sha256(true)),
    )
    .unwrap();

    assert_eq!(result.scanned, 0);
    assert_eq!(result.checked_resources, 0);
    assert_eq!(result.missing, 0);
    assert_eq!(result.orphaned, 0);
    assert!(result.issues.is_empty());
}

#[test]
fn stream_upload_resource_content_detects_most_specific_kind() {
    let (service, repository, _) = service();
    let key = StorageKey::new("docs/readme.md").unwrap();

    let resource = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command("readme", key, Bytes::from_static(b"# Readme"))
                .with_mime_type("text/plain"),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();

    assert!(saved.kind().is("azvs:markdown"));
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

    let error = block_on(
        service
            .content()
            .upload_resource_content_stream(stream_upload_command(
                "image",
                key,
                Bytes::from_static(b"new"),
            )),
    )
    .unwrap_err();

    match error {
        CoreError::Conflict { message } => assert!(message.contains("already exists")),
        other => panic!("expected storage key conflict, got {other:?}"),
    }
    assert!(repository.is_empty());
}

#[test]
fn stream_upload_resource_content_rejects_reserved_storage_key() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new(".asset-hub/action-effects/user-file.txt").unwrap();

    let error = block_on(
        service
            .content()
            .upload_resource_content_stream(stream_upload_command(
                "file",
                key.clone(),
                Bytes::from_static(b"data"),
            )),
    )
    .unwrap_err();

    match error {
        CoreError::Configuration { message } => assert!(message.contains("reserved")),
        other => panic!("expected reserved key configuration error, got {other:?}"),
    }
    assert!(repository.is_empty());
    assert!(!blob_storage.contains(&key));
}

#[test]
fn import_resource_content_rejects_reserved_storage_key() {
    let (service, repository, _) = service();

    let error = block_on(
        service.content().import_resource_content(
            ImportResourceContent::new("user-file.txt", 4)
                .with_directory(ResourceDirectory::from_path(".asset-hub/action-effects").unwrap()),
        ),
    )
    .unwrap_err();

    match error {
        CoreError::Configuration { message } => assert!(message.contains("reserved")),
        other => panic!("expected reserved key configuration error, got {other:?}"),
    }
    assert!(repository.is_empty());
}

#[test]
fn create_resource_rejects_unsupported_kind() {
    let (service, repository, _) = service_with_registry(Arc::new(
        InMemoryResourceKindRegistry::with_definitions(vec![ResourceKindDefinition::new(
            ResourceKind::default(),
            "Unknown",
            true,
        )]),
    ));

    let error = block_on(
        service
            .commands()
            .create_resource(CreateResource::new("image").with_kind("plugin:not-installed")),
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
fn stream_upload_resource_content_stream_writes_chunks_and_records_size() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/large.bin").unwrap();
    let data: BlobByteStream = Box::pin(futures_util::stream::iter([
        Ok(Bytes::from_static(b"large ")),
        Ok(Bytes::from_static(b"file ")),
        Ok(Bytes::from_static(b"bytes")),
    ]));

    let resource = block_on(
        service.content().upload_resource_content_stream(
            UploadResourceContentStream::new("large.bin", data)
                .with_directory(ResourceDirectory::from_path("assets").unwrap())
                .with_kind("asset:binary")
                .with_mime_type("application/octet-stream"),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();
    let content = saved.content().unwrap();

    assert_eq!(saved.storage_key(), key);
    assert_eq!(content.size(), 16);
    assert_eq!(content.mime_type(), Some("application/octet-stream"));
    assert_eq!(
        blob_storage.get_sync(&key),
        Some(Bytes::from_static(b"large file bytes"))
    );
}

#[test]
fn stream_upload_resource_content_rejects_kind_without_content_support() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/readme.md").unwrap();

    let error = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command("readme", key.clone(), Bytes::from_static(b"hello"))
                .with_kind("doc:markdown"),
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

    let result = block_on(
        service
            .content()
            .upload_resource_content_stream(stream_upload_command(
                "image",
                key.clone(),
                Bytes::from_static(b"image bytes"),
            )),
    );

    match result {
        Err(CoreError::Repository { operation, .. }) => assert_eq!(operation, "save"),
        other => panic!("expected repository error, got {other:?}"),
    }

    assert!(!blob_storage.contains(&key));
    assert!(repository.is_empty());
}

#[test]
fn upload_preserves_repository_error_when_compensation_delete_fails() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/compensation.bin").unwrap();
    repository.fail_next_save();
    blob_storage.fail_next_delete();

    let error = block_on(
        service
            .content()
            .upload_resource_content_stream(stream_upload_command(
                "file",
                key.clone(),
                Bytes::from_static(b"data"),
            )),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        CoreError::Repository {
            operation: "save",
            ..
        }
    ));
    assert!(blob_storage.contains(&key));
}

#[test]
fn get_resource_content_reads_existing_blob() {
    let (service, _, _) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let resource = block_on(
        service
            .content()
            .upload_resource_content_stream(stream_upload_command("image", key, data.clone())),
    )
    .unwrap();

    let content = block_on(service.content().get_resource_content(&resource.id())).unwrap();

    assert_eq!(content, Some(data));
}

#[test]
fn read_resource_returns_text_for_reader_kind() {
    let (service, _, _) = service();
    let key = StorageKey::new("books/book.txt").unwrap();
    let resource = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command("book", key, Bytes::from_static(b"Hello book"))
                .with_kind("core:document"),
        ),
    )
    .unwrap();

    let readable = block_on(service.previews().read_resource(&resource.id()))
        .unwrap()
        .unwrap();

    assert_eq!(readable.kind().as_str(), "core:document");
    assert_eq!(
        readable.view(),
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
        service.content().upload_resource_content_stream(
            stream_upload_command("note.md", key.clone(), Bytes::from_static(b"# Old"))
                .with_kind("core:document")
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();

    let output = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new("azvs.markdown.update")
                .with_input(json!({"markdown": "# New\n\nUpdated."})),
        ),
    )
    .unwrap()
    .unwrap();

    assert_eq!(output.action().as_str(), "azvs.markdown.update");
    let updated = repository.find_sync(&resource.id()).unwrap();
    let content = updated.content().unwrap();
    assert!(blob_storage.contains(&key));
    assert_eq!(updated.storage_key(), key);
    assert!(!blob_storage.contains_fragment(".asset-hub/"));
    assert!(!blob_storage.contains_fragment(".action-replacements/"));
    assert!(!blob_storage.contains_fragment(".action-backups/"));
    assert_eq!(
        blob_storage.get_sync(&updated.storage_key()).unwrap(),
        Bytes::from_static(b"# New\n\nUpdated.")
    );
    assert_eq!(content.size(), 15);
    assert_eq!(content.mime_type(), Some("text/markdown"));
    assert_eq!(content.checksum().kind(), ChecksumKind::Sha256);
    assert_eq!(content.checksum().value(), hex_sha256(b"# New\n\nUpdated."));
}

#[test]
fn write_action_scratch_content_uses_reserved_namespace() {
    let (service, _, blob_storage) = service();
    let key = StorageKey::new("docs/note.md").unwrap();
    let resource = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command("note.md", key, Bytes::from_static(b"# Old"))
                .with_kind("core:document")
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    blob_storage.fail_next_delete();

    block_on(service.actions().execute_resource_action(
        &resource.id(),
        ExecuteResourceAction::new("azvs.markdown.update").with_input(json!({"markdown": "# New"})),
    ))
    .unwrap()
    .unwrap();

    assert!(blob_storage.contains_fragment(".asset-hub/action-effects/action-replacements/"));
    assert!(!blob_storage.contains_fragment("docs/note.md.action-replacements/"));
    assert!(!blob_storage.contains_fragment("docs/note.md.action-backups/"));
}

#[test]
fn read_resource_rejects_non_reader_kind() {
    let (service, _, _) = service();
    let key = StorageKey::new("files/file.txt").unwrap();
    let resource = block_on(service.content().upload_resource_content_stream(
        stream_upload_command("file", key, Bytes::from_static(b"hello")).with_kind("asset:binary"),
    ))
    .unwrap();

    let error = block_on(service.previews().read_resource(&resource.id())).unwrap_err();

    match error {
        CoreError::Configuration { message } => {
            assert!(message.contains("does not support action `read`"))
        }
        other => panic!("expected configuration error, got {other:?}"),
    }
}

#[test]
fn describe_resource_actions_uses_declared_actions_without_format_sniffing() {
    let (service, _, _) = service();
    let pdf = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command(
                "book",
                StorageKey::new("books/book.pdf").unwrap(),
                Bytes::from_static(b"%PDF-1.4"),
            )
            .with_kind("core:document")
            .with_mime_type("application/pdf"),
        ),
    )
    .unwrap();
    let text = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command(
                "book",
                StorageKey::new("books/book.txt").unwrap(),
                Bytes::from_static(b"hello"),
            )
            .with_kind("core:document")
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

    assert!(has_action(&pdf_actions, "download_content"));
    assert!(has_action(&pdf_actions, "read"));
    assert!(!has_action(&pdf_actions, "view_inline"));
    assert!(has_action(&text_actions, "download_content"));
    assert!(has_action(&text_actions, "read"));
    assert!(!has_action(&text_actions, "view_inline"));
}

#[test]
fn core_video_resources_use_builtin_preview_for_common_video_formats() {
    let (service, _, _) = service();
    let mp4 = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command(
                "demo.mp4",
                StorageKey::new("videos/demo.mp4").unwrap(),
                Bytes::from_static(b"mp4"),
            )
            .with_kind("core:video")
            .with_mime_type("video/mp4"),
        ),
    )
    .unwrap();
    let webm = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command(
                "demo.webm",
                StorageKey::new("videos/demo.webm").unwrap(),
                Bytes::from_static(b"webm"),
            )
            .with_kind("core:video")
            .with_mime_type("video/webm"),
        ),
    )
    .unwrap();

    let mp4_actions = service.actions().describe_resource_actions(&mp4).unwrap();
    let webm_actions = service.actions().describe_resource_actions(&webm).unwrap();

    assert!(
        mp4_actions
            .available_actions()
            .iter()
            .any(|action| action.id().as_str() == ResourceAction::PREVIEW)
    );
    assert!(
        webm_actions
            .available_actions()
            .iter()
            .any(|action| action.id().as_str() == ResourceAction::PREVIEW)
    );
}

#[test]
fn preview_resource_returns_pdf_content_for_preview_kind() {
    let (service, _, _) = service();
    let resource = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command(
                "book",
                StorageKey::new("books/book.pdf").unwrap(),
                Bytes::from_static(b"%PDF-1.4"),
            )
            .with_kind("core:document")
            .with_mime_type("application/pdf"),
        ),
    )
    .unwrap();

    let preview = block_on(service.previews().preview_resource(&resource.id()))
        .unwrap()
        .unwrap();

    assert_eq!(preview.content_type(), "application/pdf");
    assert_eq!(preview.content().as_ref(), b"%PDF-1.4");
}

#[test]
fn thumbnail_resource_returns_image_content_for_thumbnail_kind() {
    let (service, _, _) = service();
    let image = Bytes::from_static(b"fake-image");
    let resource = block_on(
        service.content().upload_resource_content_stream(
            stream_upload_command(
                "image",
                StorageKey::new("images/pixel.png").unwrap(),
                image.clone(),
            )
            .with_kind("core:image")
            .with_mime_type("image/png"),
        ),
    )
    .unwrap();

    let thumbnail = block_on(service.previews().thumbnail_resource(&resource.id()))
        .unwrap()
        .unwrap();

    assert_eq!(thumbnail.content_type(), "image/png");
    assert_eq!(thumbnail.content(), &image);
}

#[test]
fn soft_delete_resource_moves_blob_to_trash_and_hides_content_read() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let resource = block_on(
        service
            .content()
            .upload_resource_content_stream(stream_upload_command(
                "image",
                key.clone(),
                data.clone(),
            )),
    )
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
    let (service, _, blob_storage) = service();
    let key = StorageKey::new("assets/restored.png").unwrap();
    let data = Bytes::from_static(b"restored bytes");
    let resource = block_on(service.content().upload_resource_content_stream(
        stream_upload_command("restored", key.clone(), data.clone()),
    ))
    .unwrap();
    let trash_key = StorageKey::new(format!(".asset-hub/trash/{}", resource.id())).unwrap();
    let deleted = block_on(service.commands().soft_delete_resource(&resource.id()))
        .unwrap()
        .unwrap();

    let restored = block_on(
        service
            .commands()
            .update_resource_snapshot(deleted, UpdateResource::new().with_restore(true)),
    )
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
    let resource = block_on(service.content().upload_resource_content_stream(
        stream_upload_command("concurrent-delete", key.clone(), data.clone()),
    ))
    .unwrap();
    let stale = resource.clone();
    let trash_key = StorageKey::new(format!(".asset-hub/trash/{}", resource.id())).unwrap();
    let mut concurrent = resource;
    concurrent
        .set_description(Some("concurrent".to_owned()))
        .unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(service.commands().soft_delete_resource_snapshot(stale)).unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(data));
    assert!(!blob_storage.contains(&trash_key));
    assert!(!repository.find_sync(&concurrent.id()).unwrap().is_deleted());
}

#[test]
fn remove_resource_deletes_blob_and_repository_record() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let resource = block_on(service.content().upload_resource_content_stream(
        stream_upload_command("image", key.clone(), Bytes::from_static(b"image bytes")),
    ))
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
    let resource = block_on(service.content().upload_resource_content_stream(
        stream_upload_command("image", key.clone(), Bytes::from_static(b"image bytes")),
    ))
    .unwrap();
    let stale = resource.clone();
    let mut concurrent = resource;
    concurrent.rename("moved by another request").unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(service.commands().remove_resource_snapshot(stale)).unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert!(repository.find_sync(&concurrent.id()).is_some());
    assert!(blob_storage.contains(&key));
}
