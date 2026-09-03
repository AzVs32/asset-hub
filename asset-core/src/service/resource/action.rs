//! Host-side Resource action orchestration.

use super::content::{build_verified_content, calculate_checksum};
use super::{ContentService, ExecuteResourceAction, ResourceActions, ResourceService};
use crate::CoreError;
use crate::domain::{
    AccessContext, ActionAccess, Directory, DirectoryActionDefinition, DirectoryActionId,
    DirectoryId, DirectoryKind, DirectoryOperation, Resource, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionId, ResourceActionPolicy, ResourceContentEditPolicy,
    ResourceId, ResourceKind, StorageKey,
};
use crate::port::{
    DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRegistry,
    DirectoryActionRequest, LocatedResource, ResourceActionExecutor, ResourceActionOutput,
    ResourceActionRegistry, ResourceActionRequest,
};
use crate::service::{
    AuthorizationService, DirectoryActions, DirectoryService, ExecuteDirectoryAction,
    ExecutedDirectoryAction, IdempotencyOutcome, IdempotencyService, UpdateDirectory, request_hash,
    validate_action_revision,
};
use asset_plugin_api::manifest::RESOURCE_EDIT_CAPABILITY;
use asset_plugin_api::protocol::{
    DirectoryActionEffect, PluginResourceActionEffect, PluginResourceActionOutput,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Clone)]
pub struct ActionOrchestrator {
    resources: ResourceService,
    content: ContentService,
    directories: DirectoryService,
    registry: Arc<dyn ResourceActionRegistry>,
    executor: Arc<dyn ResourceActionExecutor>,
    directory_registry: Arc<dyn DirectoryActionRegistry>,
    directory_executor: Arc<dyn DirectoryActionExecutor>,
    action_policy: Arc<ResourceActionPolicy>,
    edit_policy: Arc<ResourceContentEditPolicy>,
    idempotency: IdempotencyService,
}

impl ActionOrchestrator {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        resources: ResourceService,
        content: ContentService,
        directories: DirectoryService,
        registry: Arc<dyn ResourceActionRegistry>,
        executor: Arc<dyn ResourceActionExecutor>,
        directory_registry: Arc<dyn DirectoryActionRegistry>,
        directory_executor: Arc<dyn DirectoryActionExecutor>,
        action_policy: Arc<ResourceActionPolicy>,
        edit_policy: Arc<ResourceContentEditPolicy>,
        idempotency: IdempotencyService,
    ) -> Self {
        Self {
            resources,
            content,
            directories,
            registry,
            executor,
            directory_registry,
            directory_executor,
            action_policy,
            edit_policy,
            idempotency,
        }
    }

    pub fn secured<'a>(
        &'a self,
        authorization: &'a AuthorizationService,
        context: &'a AccessContext,
    ) -> SecuredActionOrchestrator<'a> {
        SecuredActionOrchestrator {
            service: self,
            authorization,
            context,
        }
    }

    pub fn describe_kind_actions(&self, kind: &ResourceKind) -> Vec<ResourceActionDefinition> {
        self.registry
            .actions_for_kinds(&self.resources.kind_registry.lineage(kind))
    }

    pub(crate) fn max_inline_content_bytes(&self) -> u64 {
        self.action_policy.max_inline_content_bytes()
    }

    pub fn describe_resource_actions(
        &self,
        resource: &Resource,
    ) -> Result<ResourceActions, CoreError> {
        self.require_kind(resource.kind())?;
        Ok(ResourceActions::new(self.available_actions(resource)))
    }

    pub async fn execute(
        &self,
        id: &ResourceId,
        command: ExecuteResourceAction,
    ) -> Result<Option<ResourceActionOutput>, CoreError> {
        let Some(key) = command.idempotency_key().cloned() else {
            return self.execute_resource(id, command).await;
        };
        let hash = request_hash(&serde_json::json!({
            "resource_id": id.to_string(),
            "action": command.action.to_string(),
            "expected_revision": command.expected_revision,
            "input": &command.input,
        }));
        match self.idempotency.begin(&key, &hash).await? {
            IdempotencyOutcome::Execute => {
                let result = self.execute_resource(id, command).await;
                match &result {
                    Ok(Some(output)) => {
                        let _ = self
                            .idempotency
                            .complete(&key, resource_action_result(id, output)?)
                            .await;
                    }
                    _ => {
                        let _ = self.idempotency.abandon(&key).await;
                    }
                }
                result
            }
            IdempotencyOutcome::Replay(result) => {
                self.replay_resource_action(&result).await.map(Some)
            }
            IdempotencyOutcome::Conflict => Err(CoreError::conflict(format!(
                "idempotency key `{key}` was already used for a different request"
            ))),
        }
    }

    async fn execute_resource(
        &self,
        id: &ResourceId,
        command: ExecuteResourceAction,
    ) -> Result<Option<ResourceActionOutput>, CoreError> {
        let Some(resource) = self.resources.get(id).await? else {
            return Ok(None);
        };
        self.execute_snapshot(resource, command).await.map(Some)
    }

    async fn replay_resource_action(
        &self,
        result: &serde_json::Value,
    ) -> Result<ResourceActionOutput, CoreError> {
        let resource_id = result
            .get("resource_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CoreError::invariant("idempotency result is missing `resource_id`"))?;
        let resource_id = std::str::FromStr::from_str(resource_id)
            .map_err(|error| CoreError::invariant(format!("invalid stored resource id: {error}")))?;
        let action = result
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CoreError::invariant("idempotency result is missing `action`"))?;
        let action = ResourceActionId::new(action.to_string())
            .map_err(|error| CoreError::invariant(format!("invalid stored action id: {error}")))?;
        let output = result
            .get("output")
            .cloned()
            .ok_or_else(|| CoreError::invariant("idempotency result is missing `output`"))?;
        let output = serde_json::from_value::<PluginResourceActionOutput>(output)
            .map_err(|error| CoreError::invariant(format!("invalid stored action output: {error}")))?;
        Ok(ResourceActionOutput::new(resource_id, action, output))
    }

    async fn execute_snapshot(
        &self,
        located: LocatedResource,
        command: ExecuteResourceAction,
    ) -> Result<ResourceActionOutput, CoreError> {
        let action = self.resolve_action(located.resource(), &command.action)?;
        validate_action_revision(
            action.access(),
            command.expected_revision,
            located.resource().revision(),
            "resource",
            located.resource().id().to_string(),
        )?;
        let storage_key = located.storage_key()?;
        let content = self.load_action_content(&located, &action).await?;
        let content_delivery = located
            .resource()
            .content()
            .and_then(|content| {
                resolved_content_delivery(&action, content.size(), &self.action_policy)
            })
            .unwrap_or(ResourceActionContentDelivery::Auto);
        let request = ResourceActionRequest::new(
            located.resource().clone(),
            located.directory().clone(),
            storage_key.clone(),
            command.action.clone(),
            action.access(),
            command.input,
        )
        .with_content(content_delivery, content);
        let output = self.executor.execute(request).await?;
        self.validate_output(located.resource().id(), &command.action, &action, &output)?;
        self.apply_effects(located, &storage_key, &output, action.access())
            .await?;
        Ok(output)
    }

    fn resolve_action(
        &self,
        resource: &Resource,
        action_id: &ResourceActionId,
    ) -> Result<ResourceActionDefinition, CoreError> {
        self.require_kind(resource.kind())?;
        self.available_actions(resource)
            .into_iter()
            .find(|action| action.id().as_str() == action_id.as_str())
            .ok_or_else(|| CoreError::unsupported("resource action", action_id.to_string()))
    }

    fn require_kind(&self, kind: &ResourceKind) -> Result<(), CoreError> {
        self.resources
            .kind_registry
            .get(kind)
            .map(|_| ())
            .ok_or_else(|| {
                CoreError::invariant(format!(
                    "persisted resource kind `{kind}` is not registered"
                ))
            })
    }

    fn available_actions(&self, resource: &Resource) -> Vec<ResourceActionDefinition> {
        let lineage = self.resources.kind_registry.lineage(resource.kind());
        let content = resource.content();
        let applicable = self
            .registry
            .action_candidates_for_kinds(&lineage)
            .into_iter()
            .filter(|action| content.is_some() || !action.requirements().content)
            .filter(|action| {
                lineage.iter().any(|kind| {
                    action.matches_resource(
                        kind.as_str(),
                        content.and_then(|content| content.mime_type()),
                        content.map(|_| resource.name()),
                    )
                })
            })
            .filter(|action| {
                let is_edit = action
                    .provides()
                    .is_some_and(|capability| capability.as_str() == RESOURCE_EDIT_CAPABILITY);
                !is_edit
                    || content
                        .is_some_and(|content| content.size() <= self.edit_policy.max_text_bytes())
            })
            .collect();
        self.registry.resolve_capability_providers(applicable)
    }

    async fn load_action_content(
        &self,
        located: &LocatedResource,
        action: &ResourceActionDefinition,
    ) -> Result<Option<Bytes>, CoreError> {
        let Some(content) = located.resource().content() else {
            return Ok(None);
        };
        if !matches!(
            resolved_content_delivery(action, content.size(), &self.action_policy),
            Some(ResourceActionContentDelivery::Inline)
        ) {
            return Ok(None);
        }
        if content.size() > self.action_policy.max_content_bytes() {
            return Err(CoreError::limit_exceeded(
                "plugin action content",
                self.action_policy.max_content_bytes(),
                content.size(),
            ));
        }
        self.content.get_resource_content_snapshot(located).await
    }

    fn validate_output(
        &self,
        resource_id: ResourceId,
        action_id: &ResourceActionId,
        action: &ResourceActionDefinition,
        output: &ResourceActionOutput,
    ) -> Result<(), CoreError> {
        if output.resource_id() != resource_id || output.action() != action_id {
            return Err(CoreError::invariant(
                "resource action returned output for a different invocation",
            ));
        }
        if let Some(view) = &output.output().view
            && !action.output().views.iter().any(|kind| kind == view.kind())
        {
            return Err(CoreError::invariant(format!(
                "action `{action_id}` returned an undeclared view"
            )));
        }
        if output.output().view.is_none() && output.output().effects.is_empty() {
            return Err(CoreError::invariant(format!(
                "action `{action_id}` returned neither a view nor an effect"
            )));
        }
        for effect in &output.output().effects {
            if !action
                .output()
                .effects
                .iter()
                .any(|kind| kind == effect.kind())
            {
                return Err(CoreError::invariant(format!(
                    "action `{action_id}` returned undeclared effect `{}`",
                    effect.kind()
                )));
            }
        }
        let replacements = output
            .output()
            .effects
            .iter()
            .filter(|effect| matches!(effect, PluginResourceActionEffect::ReplaceContent(_)))
            .count();
        if replacements > 1 {
            return Err(CoreError::invariant(
                "resource action returned multiple replace_content effects",
            ));
        }
        if output
            .output()
            .effects
            .iter()
            .any(|effect| matches!(effect, PluginResourceActionEffect::Delete))
            && output.output().effects.len() > 1
        {
            return Err(CoreError::invariant(
                "resource action combined delete with another effect",
            ));
        }
        Ok(())
    }

    async fn apply_effects(
        &self,
        located: LocatedResource,
        storage_key: &StorageKey,
        output: &ResourceActionOutput,
        access: ActionAccess,
    ) -> Result<(), CoreError> {
        if output.output().effects.is_empty() {
            return Ok(());
        }
        if access != ActionAccess::Write {
            return Err(CoreError::invariant(
                "read-only resource action returned write effects",
            ));
        }
        let mut resource = located.resource().clone();
        for effect in &output.output().effects {
            match effect {
                PluginResourceActionEffect::ReplaceContent(effect) => {
                    let current = resource.content().cloned().ok_or_else(|| {
                        CoreError::invariant("replace_content requires existing content")
                    })?;
                    let data =
                        Bytes::from(BASE64_STANDARD.decode(effect.data.as_bytes()).map_err(
                            |error| {
                                CoreError::invariant(format!(
                                    "resource action returned invalid base64: {error}"
                                ))
                            },
                        )?);
                    let content = build_verified_content(
                        data.len() as u64,
                        effect
                            .mime_type
                            .clone()
                            .or_else(|| current.mime_type().map(str::to_string)),
                        calculate_checksum(data.as_ref())?,
                        None,
                    )?;
                    self.content
                        .replace_content_bytes_snapshot(&mut resource, storage_key, content, data)
                        .await?;
                }
                PluginResourceActionEffect::Delete => {
                    self.resources
                        .delete(located.clone(), resource.revision())
                        .await?;
                }
            }
        }
        Ok(())
    }

    pub fn describe_directory_kind_actions(
        &self,
        kind: &DirectoryKind,
    ) -> Vec<DirectoryActionDefinition> {
        let lineage = self.directories.kind_lineage(kind);
        self.directory_registry.actions_for_kinds(&lineage)
    }

    pub fn describe_directory_actions(
        &self,
        directory: &Directory,
    ) -> Result<DirectoryActions, CoreError> {
        Ok(DirectoryActions::new(
            self.available_actions_for_directory(directory)?,
        ))
    }

    pub(crate) fn resolve_directory_action(
        &self,
        directory: &Directory,
        action_id: &DirectoryActionId,
    ) -> Result<DirectoryActionDefinition, CoreError> {
        self.available_actions_for_directory(directory)?
            .into_iter()
            .find(|action| action.id().as_str() == action_id.as_str())
            .ok_or_else(|| CoreError::unsupported("directory action", action_id.to_string()))
    }

    /// Resolve the authoritative action set for one Directory instance, excluding root deletion.
    fn available_actions_for_directory(
        &self,
        directory: &Directory,
    ) -> Result<Vec<DirectoryActionDefinition>, CoreError> {
        self.directories.require_kind_registered(directory.kind())?;
        Ok(self
            .describe_directory_kind_actions(directory.kind())
            .into_iter()
            .filter(|action| {
                !(directory.id().is_root()
                    && action
                        .output()
                        .effects
                        .iter()
                        .any(|effect| effect == "delete"))
            })
            .collect())
    }

    pub(crate) async fn invoke_directory_action(
        &self,
        id: &DirectoryId,
        command: ExecuteDirectoryAction,
    ) -> Result<ExecutedDirectoryAction, CoreError> {
        let located = self.directories.find_by_id(id).await?;
        let expected_revision = located.directory().revision();
        let definition = self.resolve_directory_action(located.directory(), &command.action)?;
        validate_action_revision(
            definition.access(),
            command.expected_revision,
            expected_revision,
            "directory",
            id.to_string(),
        )?;
        let output = self
            .directory_executor
            .execute(DirectoryActionRequest::new(
                located,
                command.action.clone(),
                definition.access(),
                definition.requirements().clone(),
                command.input,
            ))
            .await?;
        self.validate_directory_action_output(id, &command.action, &definition, &output)?;
        Ok(ExecutedDirectoryAction::new(
            *id,
            expected_revision,
            definition.access(),
            output,
        ))
    }

    pub(crate) async fn apply_directory_action(
        &self,
        executed: &ExecutedDirectoryAction,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<(), CoreError> {
        if executed
            .output()
            .output()
            .effects
            .iter()
            .any(|effect| matches!(effect, DirectoryActionEffect::CreateTree(_)))
        {
            return Err(CoreError::configuration(
                "create_tree directory effects require the AssetWorkflowService boundary",
            ));
        }
        self.apply_directory_effects(
            &executed.directory_id(),
            executed.expected_revision(),
            executed.access(),
            executed.output(),
            required_parent_ancestor,
        )
        .await
    }

    fn validate_directory_action_output(
        &self,
        directory_id: &DirectoryId,
        action_id: &DirectoryActionId,
        definition: &DirectoryActionDefinition,
        output: &DirectoryActionOutput,
    ) -> Result<(), CoreError> {
        if output.directory_id() != *directory_id || output.action() != action_id {
            return Err(CoreError::invariant(format!(
                "action `{action_id}` returned an output for a different invocation"
            )));
        }
        if let Some(view) = &output.output().view {
            let actual = view.kind();
            if !definition.output().views.iter().any(|view| view == actual) {
                return Err(CoreError::invariant(format!(
                    "action `{}` returned undeclared view `{actual}`",
                    definition.id()
                )));
            }
        }
        if output.output().view.is_none() && output.output().effects.is_empty() {
            return Err(CoreError::invariant(format!(
                "action `{}` returned neither a view nor an effect",
                definition.id()
            )));
        }
        if let Some(effect) = output.output().effects.iter().find(|effect| {
            !definition
                .output()
                .effects
                .iter()
                .any(|kind| kind == effect.kind())
        }) {
            return Err(CoreError::invariant(format!(
                "action `{}` returned undeclared effect `{}`",
                definition.id(),
                effect.kind()
            )));
        }
        if output.output().effects.len() > 1 {
            return Err(CoreError::invariant(format!(
                "action `{}` returned more than one directory effect",
                definition.id()
            )));
        }
        Ok(())
    }

    async fn apply_directory_effects(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
        access: ActionAccess,
        output: &DirectoryActionOutput,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<(), CoreError> {
        if output.output().effects.is_empty() {
            return Ok(());
        }
        if !matches!(access, ActionAccess::Write) {
            return Err(CoreError::invariant(format!(
                "action `{}` returned effects without write access",
                output.action()
            )));
        }
        for effect in output
            .output()
            .effects
            .iter()
            .filter_map(|effect| match effect {
                DirectoryActionEffect::CreateChild(effect) => Some(effect),
                DirectoryActionEffect::Update(_)
                | DirectoryActionEffect::CreateTree(_)
                | DirectoryActionEffect::Delete => None,
            })
        {
            let kind = effect
                .kind
                .as_ref()
                .map(|kind| DirectoryKind::try_new(kind.clone()))
                .transpose()?
                .unwrap_or_default();
            self.directories
                .create_with_kind_guarded(
                    id,
                    effect.name.clone(),
                    kind,
                    Some(expected_revision),
                    required_parent_ancestor,
                )
                .await?;
        }
        if let Some(effect) = output
            .output()
            .effects
            .iter()
            .find_map(|effect| match effect {
                DirectoryActionEffect::Update(effect) => Some(effect),
                DirectoryActionEffect::CreateChild(_)
                | DirectoryActionEffect::CreateTree(_)
                | DirectoryActionEffect::Delete => None,
            })
        {
            let mut command = UpdateDirectory::new(expected_revision);
            if let Some(name) = &effect.name {
                command = command.with_name(name.clone());
            }
            if let Some(parent_id) = &effect.parent_id {
                command = command.with_parent_id(
                    DirectoryId::from_str(parent_id)
                        .map_err(|error| CoreError::invariant(error.to_string()))?,
                );
            }
            if let Some(kind) = &effect.kind {
                command = command.with_kind(DirectoryKind::try_new(kind.clone())?);
            }
            self.directories
                .update_expected(id, command, required_parent_ancestor)
                .await?;
        }
        if output
            .output()
            .effects
            .iter()
            .any(|effect| matches!(effect, DirectoryActionEffect::Delete))
        {
            let directory = self.directories.find_by_id(id).await?;
            if !self
                .directories
                .delete_if_empty(&directory.id(), Some(expected_revision))
                .await?
            {
                return Err(CoreError::conflict(format!(
                    "directory `{id}` is not empty"
                )));
            }
        }
        Ok(())
    }
}

pub struct SecuredActionOrchestrator<'a> {
    service: &'a ActionOrchestrator,
    authorization: &'a AuthorizationService,
    context: &'a AccessContext,
}

impl SecuredActionOrchestrator<'_> {
    pub async fn execute(
        &self,
        id: &ResourceId,
        command: ExecuteResourceAction,
    ) -> Result<Option<ResourceActionOutput>, CoreError> {
        let Some(resource) = self.service.resources.get(id).await? else {
            return Ok(None);
        };
        let action = self
            .service
            .resolve_action(resource.resource(), &command.action)?;
        let operation = if action
            .output()
            .effects
            .iter()
            .any(|effect| effect == "delete")
        {
            DirectoryOperation::DeleteResource
        } else {
            DirectoryOperation::ExecuteResourceAction
        };
        self.authorization
            .require(self.context, resource.directory(), operation)
            .await?;
        self.service
            .execute_snapshot(resource, command)
            .await
            .map(Some)
    }
}

fn resource_action_result(
    id: &ResourceId,
    output: &ResourceActionOutput,
) -> Result<serde_json::Value, CoreError> {
    Ok(serde_json::json!({
        "resource_id": id.to_string(),
        "action": output.action().to_string(),
        "output": serde_json::to_value(output.output())
            .map_err(|error| CoreError::invariant(format!("action output must serialize: {error}")))?,
    }))
}

fn resolved_content_delivery(
    action: &ResourceActionDefinition,
    size: u64,
    policy: &ResourceActionPolicy,
) -> Option<ResourceActionContentDelivery> {
    if !action.requirements().content {
        return None;
    }
    match action.requirements().content_delivery {
        ResourceActionContentDelivery::Auto if size <= policy.max_inline_content_bytes() => {
            Some(ResourceActionContentDelivery::Inline)
        }
        ResourceActionContentDelivery::Auto => Some(ResourceActionContentDelivery::Reference),
        delivery => Some(delivery),
    }
}
