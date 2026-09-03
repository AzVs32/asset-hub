//! Narrow application coordinator for use cases that mutate or project both aggregates.

use super::{
    ActionOrchestrator, AuthorizationService, DirectoryService, ExecuteDirectoryAction,
    ResourceService, UploadService,
};
use crate::CoreError;
use crate::domain::{
    AccessContext, DirectoryActionId, DirectoryId, DirectoryKind, DirectoryOperation,
    DirectoryPath, ResourceId, ResourceKind,
};
use crate::port::{DirectoryActionOutput, ListResources, LocatedDirectory};
use crate::service::{IdempotencyOutcome, IdempotencyService, request_hash};
use asset_plugin_api::protocol::{
    CreateDirectoryTreeEffect, CreateTreeResourceEncoding, DirectoryActionEffect,
    PluginDirectoryActionOutput,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use std::collections::{BTreeMap, HashSet, VecDeque};

const DIRECTORY_ARCHIVE_PAGE_SIZE: u32 = 100;
const MAX_CREATE_TREE_DIRECTORIES: usize = 32;
const MAX_CREATE_TREE_RESOURCES: usize = 32;

/// Coordinates only operations whose consistency boundary spans Resource and Directory.
#[derive(Clone)]
pub struct AssetWorkflowService {
    resources: ResourceService,
    uploads: UploadService,
    actions: ActionOrchestrator,
    directories: DirectoryService,
    idempotency: IdempotencyService,
}

impl AssetWorkflowService {
    pub fn new(
        resources: ResourceService,
        uploads: UploadService,
        actions: ActionOrchestrator,
        directories: DirectoryService,
        idempotency: IdempotencyService,
    ) -> Self {
        Self {
            resources,
            uploads,
            actions,
            directories,
            idempotency,
        }
    }

    pub fn secured<'a>(
        &'a self,
        authorization: &'a AuthorizationService,
        context: &'a AccessContext,
    ) -> SecuredAssetWorkflowService<'a> {
        SecuredAssetWorkflowService {
            workflows: self,
            authorization,
            context,
        }
    }
}

/// Authorization-bound cross-aggregate operations.
pub struct SecuredAssetWorkflowService<'a> {
    workflows: &'a AssetWorkflowService,
    authorization: &'a AuthorizationService,
    context: &'a AccessContext,
}

impl SecuredAssetWorkflowService<'_> {
    async fn require(
        &self,
        directory: &crate::port::DirectoryLocation,
        operation: DirectoryOperation,
    ) -> Result<(), CoreError> {
        self.authorization
            .require(self.context, directory, operation)
            .await
    }

    pub async fn directory_archive_manifest(
        &self,
        id: &DirectoryId,
    ) -> Result<DirectoryArchiveManifest, CoreError> {
        let root = self.workflows.directories.find_by_id(id).await?;
        self.require(root.location(), DirectoryOperation::DownloadDirectory)
            .await?;
        let archive_root = if root.id().is_root() {
            "asset-hub".to_string()
        } else {
            root.directory().name().to_string()
        };
        let filename = format!("{archive_root}.zip");
        let root_path = root.path().path().to_string();
        let mut pending = VecDeque::from([root]);
        let mut directories = Vec::new();
        let mut resources = Vec::new();

        while let Some(directory) = pending.pop_front() {
            if !self
                .workflows
                .directories
                .contains(id, &directory.id())
                .await?
            {
                continue;
            }
            let archive_path =
                directory_archive_path(&archive_root, &root_path, directory.path().path())?;
            directories.push(format!("{archive_path}/"));

            let mut offset = 0;
            loop {
                let page = self
                    .workflows
                    .resources
                    .list(ListResources::new(
                        DIRECTORY_ARCHIVE_PAGE_SIZE,
                        offset,
                        directory.id(),
                    ))
                    .await?;
                let item_count = page.items.len() as u64;
                resources.extend(page.items.into_iter().filter_map(|located| {
                    let resource = located.resource();
                    resource.content().map(|content| {
                        DirectoryArchiveResource::new(
                            resource.id(),
                            format!("{archive_path}/{}", resource.name()),
                            content.size(),
                        )
                    })
                }));
                offset += item_count;
                if offset >= page.total || item_count == 0 {
                    break;
                }
            }

            pending.extend(
                self.workflows
                    .directories
                    .list_located_children(&directory.id())
                    .await?,
            );
        }

        directories.sort();
        resources.sort_by(|left, right| left.path().cmp(right.path()));
        Ok(DirectoryArchiveManifest::new(
            filename,
            directories,
            resources,
        ))
    }

    pub async fn execute_directory_action(
        &self,
        id: &DirectoryId,
        command: ExecuteDirectoryAction,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let Some(key) = command.idempotency_key().cloned() else {
            return self.execute_directory_action_inner(id, command).await;
        };
        let hash = request_hash(&serde_json::json!({
            "directory_id": id.to_string(),
            "action": command.action.to_string(),
            "expected_revision": command.expected_revision,
            "input": &command.input,
        }));
        match self.workflows.idempotency.begin(&key, &hash).await? {
            IdempotencyOutcome::Execute => {
                let result = self.execute_directory_action_inner(id, command).await;
                match &result {
                    Ok(output) => {
                        let _ = self
                            .workflows
                            .idempotency
                            .complete(&key, directory_action_result(id, output)?)
                            .await;
                    }
                    Err(_) => {
                        let _ = self.workflows.idempotency.abandon(&key).await;
                    }
                }
                result
            }
            IdempotencyOutcome::Replay(result) => self.replay_directory_action(&result).await,
            IdempotencyOutcome::Conflict => Err(CoreError::conflict(format!(
                "idempotency key `{key}` was already used for a different request"
            ))),
        }
    }

    async fn execute_directory_action_inner(
        &self,
        id: &DirectoryId,
        command: ExecuteDirectoryAction,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let directory = self.workflows.directories.find_by_id(id).await?;
        let definition = self
            .workflows
            .actions
            .resolve_directory_action(directory.directory(), &command.action)?;
        let operation = if definition
            .output()
            .effects
            .iter()
            .any(|effect| effect == "delete")
        {
            DirectoryOperation::DeleteDirectory
        } else {
            DirectoryOperation::ExecuteDirectoryAction
        };
        self.require(directory.location(), operation).await?;
        let scope_root = self
            .authorization
            .workspace_scope(self.context)
            .await?
            .root()
            .id();
        let executed = self
            .workflows
            .actions
            .invoke_directory_action(id, command)
            .await?;
        let create_tree =
            executed
                .output()
                .output()
                .effects
                .iter()
                .find_map(|effect| match effect {
                    DirectoryActionEffect::CreateTree(effect) => Some(effect.clone()),
                    _ => None,
                });
        if let Some(effect) = create_tree {
            self.apply_create_tree(&directory, executed.expected_revision(), effect, scope_root)
                .await?;
        } else {
            self.workflows
                .actions
                .apply_directory_action(&executed, Some(scope_root))
                .await?;
        }
        Ok(executed.into_output())
    }

    async fn replay_directory_action(
        &self,
        result: &serde_json::Value,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let directory_id = result
            .get("directory_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CoreError::invariant("idempotency result is missing `directory_id`"))?;
        let directory_id = std::str::FromStr::from_str(directory_id).map_err(|error| {
            CoreError::invariant(format!("invalid stored directory id: {error}"))
        })?;
        let action = result
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CoreError::invariant("idempotency result is missing `action`"))?;
        let action = DirectoryActionId::new(action.to_string())
            .map_err(|error| CoreError::invariant(format!("invalid stored action id: {error}")))?;
        let output = result
            .get("output")
            .cloned()
            .ok_or_else(|| CoreError::invariant("idempotency result is missing `output`"))?;
        let output =
            serde_json::from_value::<PluginDirectoryActionOutput>(output).map_err(|error| {
                CoreError::invariant(format!("invalid stored action output: {error}"))
            })?;
        Ok(DirectoryActionOutput::new(directory_id, action, output))
    }

    async fn apply_create_tree(
        &self,
        root: &LocatedDirectory,
        expected_revision: u64,
        effect: CreateDirectoryTreeEffect,
        scope_root: DirectoryId,
    ) -> Result<(), CoreError> {
        if effect.directories.is_empty() && effect.resources.is_empty() {
            return Err(CoreError::invalid_operation(
                "create_tree must contain a directory or resource",
            ));
        }
        if effect.directories.len() > MAX_CREATE_TREE_DIRECTORIES {
            return Err(CoreError::limit_exceeded(
                "create_tree directories",
                MAX_CREATE_TREE_DIRECTORIES as u64,
                effect.directories.len() as u64,
            ));
        }
        if effect.resources.len() > MAX_CREATE_TREE_RESOURCES {
            return Err(CoreError::limit_exceeded(
                "create_tree resources",
                MAX_CREATE_TREE_RESOURCES as u64,
                effect.resources.len() as u64,
            ));
        }
        let current = self.workflows.directories.find_by_id(&root.id()).await?;
        if current.directory().revision() != expected_revision {
            return Err(CoreError::revision_conflict(
                "directory",
                root.id().to_string(),
            ));
        }

        let mut directory_specs = effect
            .directories
            .into_iter()
            .map(|spec| {
                let path = canonical_relative_directory(&spec.path, false)?;
                let kind = spec
                    .kind
                    .map(DirectoryKind::try_new)
                    .transpose()?
                    .unwrap_or_default();
                Ok((path, kind))
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        directory_specs.sort_by_key(|(path, _)| path.path().split('/').count());
        let mut unique_directories = HashSet::new();
        for (path, _) in &directory_specs {
            if !unique_directories.insert(path.path().to_string()) {
                return Err(CoreError::conflict(format!(
                    "create_tree contains duplicate directory `{path}`"
                )));
            }
        }

        let mut prepared_resources = Vec::with_capacity(effect.resources.len());
        let mut unique_resources = HashSet::new();
        let mut total_bytes = 0_u64;
        let max_bytes = self.workflows.actions.max_inline_content_bytes();
        for spec in effect.resources {
            let directory = canonical_relative_directory(&spec.directory, true)?;
            let kind = spec.kind.map(ResourceKind::try_new).transpose()?;
            let data = match spec.encoding {
                CreateTreeResourceEncoding::Base64 => {
                    BASE64_STANDARD.decode(spec.data).map_err(|error| {
                        CoreError::invalid_operation(format!(
                            "create_tree resource `{}` contains invalid base64: {error}",
                            spec.name
                        ))
                    })?
                }
            };
            total_bytes = total_bytes.checked_add(data.len() as u64).ok_or_else(|| {
                CoreError::limit_exceeded("create_tree content", max_bytes, u64::MAX)
            })?;
            if total_bytes > max_bytes {
                return Err(CoreError::limit_exceeded(
                    "create_tree content",
                    max_bytes,
                    total_bytes,
                ));
            }
            let relative_key = if directory.is_root() {
                spec.name.clone()
            } else {
                format!("{}/{name}", directory.path(), name = spec.name)
            };
            if !unique_resources.insert(relative_key.clone()) {
                return Err(CoreError::conflict(format!(
                    "create_tree contains duplicate resource `{relative_key}`"
                )));
            }
            prepared_resources.push(PreparedTreeResource {
                directory,
                name: spec.name,
                kind,
                mime_type: spec.mime_type,
                data: Bytes::from(data),
            });
        }

        let mut locations = BTreeMap::from([(String::new(), root.location().clone())]);
        let mut created_directories = Vec::new();
        let mut created_resources = Vec::new();
        let result = async {
            for (relative, kind) in directory_specs {
                let parent = locations.get(relative.parent_path()).ok_or_else(|| {
                    CoreError::invalid_operation(format!(
                        "create_tree directory `{relative}` has an undeclared parent"
                    ))
                })?;
                let created = self
                    .workflows
                    .directories
                    .create_with_kind_in_scope(&parent.id(), relative.name(), kind, scope_root)
                    .await?;
                locations.insert(relative.path().to_string(), created.location().clone());
                created_directories.push(created);
            }
            for resource in prepared_resources {
                let directory = locations.get(resource.directory.path()).ok_or_else(|| {
                    CoreError::invalid_operation(format!(
                        "create_tree resource `{}` targets an undeclared directory `{}`",
                        resource.name, resource.directory
                    ))
                })?;
                let created = self
                    .workflows
                    .uploads
                    .create_generated(
                        directory,
                        resource.name,
                        resource.kind,
                        resource.mime_type,
                        resource.data,
                    )
                    .await?;
                created_resources.push(created);
            }
            Ok::<(), CoreError>(())
        }
        .await;

        if let Err(error) = result {
            for resource in created_resources.into_iter().rev() {
                if let Err(rollback_error) = self
                    .workflows
                    .resources
                    .delete(resource.clone(), resource.resource().revision())
                    .await
                {
                    tracing::error!(%rollback_error, "failed to roll back create_tree resource");
                }
            }
            for directory in created_directories.into_iter().rev() {
                if let Err(rollback_error) = self
                    .workflows
                    .directories
                    .delete_if_empty(&directory.id(), None)
                    .await
                {
                    tracing::error!(%rollback_error, "failed to roll back create_tree directory");
                }
            }
            return Err(error);
        }
        Ok(())
    }
}

/// Authorized, point-in-time directory tree projection used to build a ZIP download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryArchiveManifest {
    filename: String,
    directories: Vec<String>,
    resources: Vec<DirectoryArchiveResource>,
}

impl DirectoryArchiveManifest {
    fn new(
        filename: String,
        directories: Vec<String>,
        resources: Vec<DirectoryArchiveResource>,
    ) -> Self {
        Self {
            filename,
            directories,
            resources,
        }
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn directories(&self) -> &[String] {
        &self.directories
    }

    pub fn resources(&self) -> &[DirectoryArchiveResource] {
        &self.resources
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryArchiveResource {
    resource_id: ResourceId,
    path: String,
    content_length: u64,
}

impl DirectoryArchiveResource {
    fn new(resource_id: ResourceId, path: String, content_length: u64) -> Self {
        Self {
            resource_id,
            path,
            content_length,
        }
    }

    pub fn resource_id(&self) -> ResourceId {
        self.resource_id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn content_length(&self) -> u64 {
        self.content_length
    }
}

struct PreparedTreeResource {
    directory: DirectoryPath,
    name: String,
    kind: Option<ResourceKind>,
    mime_type: Option<String>,
    data: Bytes,
}

fn directory_action_result(
    id: &DirectoryId,
    output: &DirectoryActionOutput,
) -> Result<serde_json::Value, CoreError> {
    Ok(serde_json::json!({
        "directory_id": id.to_string(),
        "action": output.action().to_string(),
        "output": serde_json::to_value(output.output())
            .map_err(|error| CoreError::invariant(format!("action output must serialize: {error}")))?,
    }))
}

fn canonical_relative_directory(
    value: &str,
    allow_empty: bool,
) -> Result<DirectoryPath, CoreError> {
    let path = DirectoryPath::from_path(value.to_string())?;
    if path.path() != value || (!allow_empty && path.is_root()) {
        return Err(CoreError::invalid_operation(format!(
            "create_tree path `{value}` must be canonical{}",
            if allow_empty { "" } else { " and non-empty" }
        )));
    }
    Ok(path)
}

fn directory_archive_path(
    archive_root: &str,
    root_path: &str,
    directory_path: &str,
) -> Result<String, CoreError> {
    if directory_path == root_path {
        return Ok(archive_root.to_string());
    }
    let relative = if root_path.is_empty() {
        directory_path
    } else {
        directory_path
            .strip_prefix(root_path)
            .and_then(|suffix| suffix.strip_prefix('/'))
            .ok_or_else(|| CoreError::invariant("directory left the archive subtree"))?
    };
    Ok(format!("{archive_root}/{relative}"))
}
