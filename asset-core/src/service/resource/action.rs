//! Host-side Resource action orchestration.

use super::content::{build_verified_content, calculate_checksum};
use super::{ContentService, ExecuteResourceAction, ResourceActions, ResourceService};
use crate::CoreError;
use crate::domain::{
    AccessContext, ActionAccess, DirectoryOperation, Resource, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionId, ResourceActionPolicy, ResourceContentEditPolicy,
    ResourceId, ResourceKind, StorageKey,
};
use crate::port::{
    BlobStorage, LocatedResource, ResourceActionExecutor, ResourceActionOutput,
    ResourceActionRegistry, ResourceActionRequest,
};
use crate::service::{AuthorizationService, validate_action_revision};
use asset_plugin_api::manifest::RESOURCE_EDIT_CAPABILITY;
use asset_plugin_api::protocol::PluginResourceActionEffect;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use std::sync::Arc;

#[derive(Clone)]
pub struct ActionOrchestrator {
    resources: ResourceService,
    content: ContentService,
    blob_storage: Arc<dyn BlobStorage>,
    registry: Arc<dyn ResourceActionRegistry>,
    executor: Arc<dyn ResourceActionExecutor>,
    action_policy: Arc<ResourceActionPolicy>,
    edit_policy: Arc<ResourceContentEditPolicy>,
}

impl ActionOrchestrator {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        resources: ResourceService,
        content: ContentService,
        blob_storage: Arc<dyn BlobStorage>,
        registry: Arc<dyn ResourceActionRegistry>,
        executor: Arc<dyn ResourceActionExecutor>,
        action_policy: Arc<ResourceActionPolicy>,
        edit_policy: Arc<ResourceContentEditPolicy>,
    ) -> Self {
        Self {
            resources,
            content,
            blob_storage,
            registry,
            executor,
            action_policy,
            edit_policy,
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
        let Some(resource) = self.resources.get(id).await? else {
            return Ok(None);
        };
        self.execute_snapshot(resource, command).await.map(Some)
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
        let content = self
            .load_action_content(located.resource(), &storage_key, &action)
            .await?;
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
        resource: &Resource,
        storage_key: &StorageKey,
        action: &ResourceActionDefinition,
    ) -> Result<Option<Bytes>, CoreError> {
        let Some(content) = resource.content() else {
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
        self.blob_storage.get(storage_key).await
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
                    let data = Bytes::from(
                        BASE64_STANDARD.decode(effect.data.as_bytes()).map_err(|error| {
                            CoreError::invariant(format!(
                                "resource action returned invalid base64: {error}"
                            ))
                        })?,
                    );
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
                        .replace_content_bytes_snapshot(
                            &mut resource,
                            storage_key,
                            content,
                            data,
                        )
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
        let operation = if action.output().effects.iter().any(|effect| effect == "delete") {
            DirectoryOperation::DeleteResource
        } else {
            DirectoryOperation::ExecuteResourceAction
        };
        self.authorization
            .require(self.context, resource.directory(), operation)
            .await?;
        self.service.execute_snapshot(resource, command).await.map(Some)
    }
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
