//! 资源动作服务。
//!
//! 本模块负责把资源、kind/action 声明和动作执行器连接起来：解析可用动作、执行声明动作，并应用动作返回的写入效果。

use super::content::{build_verified_content, calculate_checksum};
use super::{ExecuteResourceAction, ResourceActions, ResourceService};
use crate::CoreError;
use crate::domain::{
    ActionAccess, Resource, ResourceActionContentDelivery, ResourceActionDefinition,
    ResourceActionId, ResourceActionPolicy, ResourceContent, ResourceId, StorageKey,
};
use crate::port::{LocatedResource, ResourceActionOutput, ResourceActionRequest};
use crate::service::validate_action_revision;
use asset_plugin_api::protocol::PluginResourceActionEffect;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;

/// 资源动作服务。
///
/// 动作服务不决定 HTTP 表达形式，只返回核心层的动作输出和资源能力描述。
pub(super) struct ResourceActionService<'a> {
    service: &'a ResourceService,
}

impl<'a> ResourceActionService<'a> {
    /// 创建资源动作服务。
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    #[cfg(test)]
    pub(super) async fn execute_resource_action(
        &self,
        id: &ResourceId,
        command: ExecuteResourceAction,
    ) -> Result<Option<ResourceActionOutput>, CoreError> {
        let Some(resource) = self.service.commands().find_resource(id).await? else {
            return Ok(None);
        };
        self.execute_resource_action_snapshot(resource, command)
            .await
            .map(Some)
    }

    /// 计算资源当前可执行动作。
    ///
    /// 该方法统一封装资源内容状态和注册 kind 能力，供不同应用入口复用，
    /// 避免在 HTTP、CLI、TUI 中重复拼装判断逻辑。
    pub(super) fn describe_resource_actions(
        &self,
        resource: &Resource,
    ) -> Result<ResourceActions, CoreError> {
        self.service.require_kind_definition(resource.kind())?;
        if resource.is_deleted() {
            return Ok(ResourceActions::default());
        }

        let available_actions = self.service.available_actions_for_resource(resource);

        Ok(ResourceActions::new(available_actions))
    }

    /// 执行资源类型声明的插件动作。
    ///
    /// 核心负责资源存在性、删除状态、kind/action 声明、访问边界和对象内容加载；具体 wasm
    /// 运行时由 `ResourceActionExecutor` 端口承接。
    pub(super) async fn execute_resource_action_snapshot(
        &self,
        resource: LocatedResource,
        command: ExecuteResourceAction,
    ) -> Result<ResourceActionOutput, CoreError> {
        let definition =
            self.resolve_declared_resource_action(resource.resource(), &command.action)?;
        validate_action_revision(
            definition.access(),
            command.expected_revision,
            resource.resource().revision(),
            "resource",
            resource.resource().id().to_string(),
        )?;
        self.execute_declared_resource_action_snapshot(
            resource,
            command.action,
            command.input,
            definition,
        )
        .await
    }

    pub(super) async fn execute_declared_resource_action_snapshot(
        &self,
        located: LocatedResource,
        action_id: ResourceActionId,
        input: serde_json::Value,
        action: ResourceActionDefinition,
    ) -> Result<ResourceActionOutput, CoreError> {
        let (mut resource, directory) = located.into_parts();
        // 1. Load content only when the resolved action contract says the executor should receive
        //    it. Resolution and revision validation happened before touching runtime state.
        let storage_key = StorageKey::from_resource_path(directory.path(), resource.name())?;
        let content = self
            .load_declared_resource_action_content(&resource, &storage_key, &action)
            .await?;

        // 2. Dispatch the request through the configured action executor.
        let access = action.access();
        let Some(ports) = &self.service.action_ports else {
            return Err(CoreError::configuration(
                "resource action executor is not configured",
            ));
        };
        let content_delivery = resource
            .content()
            .and_then(|content| {
                resolved_content_delivery(
                    &action,
                    content.size(),
                    &self.service.resource_action_policy,
                )
            })
            .unwrap_or(ResourceActionContentDelivery::Auto);
        let request = ResourceActionRequest::new(
            resource.clone(),
            directory.clone(),
            storage_key.clone(),
            action_id.clone(),
            access,
            input,
        )
        .with_content(content_delivery, content);
        let output = ports.executor.execute(request).await?;
        self.validate_action_output(resource.id(), &action_id, &action, &output)?;

        // 3. Apply write effects after the executor returns, guarded by the action access boundary.
        self.apply_action_effects(&mut resource, &directory, &storage_key, &output, access)
            .await?;

        Ok(output)
    }

    pub(super) fn resolve_declared_resource_action(
        &self,
        resource: &Resource,
        action_id: &ResourceActionId,
    ) -> Result<ResourceActionDefinition, CoreError> {
        self.service.require_kind_definition(resource.kind())?;
        if resource.is_deleted() {
            return Err(CoreError::invalid_operation(format!(
                "deleted resource `{}` cannot execute actions",
                resource.id()
            )));
        }
        let declared_actions = self.service.available_actions_for_resource(resource);
        declared_actions
            .into_iter()
            .find(|action| action.id().as_str() == action_id.as_str())
            .ok_or_else(|| CoreError::unsupported("resource action", action_id.to_string()))
    }

    fn validate_action_output(
        &self,
        resource_id: ResourceId,
        action_id: &ResourceActionId,
        action: &ResourceActionDefinition,
        output: &ResourceActionOutput,
    ) -> Result<(), CoreError> {
        if output.resource_id() != resource_id || output.action() != action_id {
            return Err(CoreError::invariant(format!(
                "action `{action_id}` returned an output for a different invocation"
            )));
        }
        if let Some(view) = &output.output().view {
            let actual = view.kind();
            if !action
                .output()
                .views
                .iter()
                .any(|declared| declared == actual)
            {
                return Err(CoreError::invariant(format!(
                    "action `{}` returned undeclared view `{actual}`",
                    action.id()
                )));
            }
        }
        if output.output().view.is_none() && output.output().effects.is_empty() {
            return Err(CoreError::invariant(format!(
                "action `{}` returned neither a view nor an effect",
                action.id()
            )));
        }
        if let Some(effect) = output.output().effects.iter().find(|effect| {
            !action
                .output()
                .effects
                .iter()
                .any(|kind| kind == effect.kind())
        }) {
            return Err(CoreError::invariant(format!(
                "action `{}` returned undeclared effect `{}`",
                action.id(),
                effect.kind()
            )));
        }
        let replacements = output
            .output()
            .effects
            .iter()
            .filter(|effect| matches!(effect, PluginResourceActionEffect::ReplaceContent(_)))
            .count();
        if replacements > 1 {
            return Err(CoreError::invariant(format!(
                "action `{}` returned more than one replace_content effect",
                action.id()
            )));
        }
        if output
            .output()
            .effects
            .iter()
            .any(|effect| matches!(effect, PluginResourceActionEffect::Delete))
            && output.output().effects.len() > 1
        {
            return Err(CoreError::invariant(format!(
                "action `{}` combined delete with another resource effect",
                action.id()
            )));
        }
        Ok(())
    }

    async fn load_declared_resource_action_content(
        &self,
        resource: &Resource,
        storage_key: &StorageKey,
        action: &ResourceActionDefinition,
    ) -> Result<Option<Bytes>, CoreError> {
        let Some(content_ref) = resource.content() else {
            return Ok(None);
        };
        if !should_load_declared_action_content(
            action,
            content_ref,
            &self.service.resource_action_policy,
        ) {
            return Ok(None);
        }
        let max_content_bytes = self.service.resource_action_policy.max_content_bytes();
        if content_ref.size() > max_content_bytes {
            return Err(CoreError::limit_exceeded(
                "plugin action content",
                max_content_bytes,
                content_ref.size(),
            ));
        }

        self.service.blob_storage.get(storage_key).await
    }

    async fn apply_action_effects(
        &self,
        resource: &mut Resource,
        directory: &crate::port::DirectoryLocation,
        storage_key: &StorageKey,
        output: &ResourceActionOutput,
        access: ActionAccess,
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

        for effect in &output.output().effects {
            match effect {
                PluginResourceActionEffect::ReplaceContent(effect) => {
                    let Some(current_content) = resource.content().cloned() else {
                        return Err(CoreError::invariant(format!(
                            "action `{}` cannot replace missing resource content",
                            output.action()
                        )));
                    };
                    let data = BASE64_STANDARD
                        .decode(effect.data.as_bytes())
                        .map(Bytes::from)
                        .map_err(|error| {
                            CoreError::invariant(format!(
                                "action `{}` returned invalid replace_content base64: {error}",
                                output.action()
                            ))
                        })?;
                    let checksum = calculate_checksum(data.as_ref())?;
                    let content = build_verified_content(
                        data.len() as u64,
                        effect
                            .mime_type
                            .clone()
                            .or_else(|| current_content.mime_type().map(str::to_string)),
                        checksum,
                        None,
                    )?;
                    self.service
                        .content()
                        .replace_content_bytes_snapshot(resource, storage_key, content, data)
                        .await?;
                }
                PluginResourceActionEffect::Delete => {
                    *resource = self
                        .service
                        .commands()
                        .soft_delete_resource_snapshot(LocatedResource::new(
                            resource.clone(),
                            directory.clone(),
                        )?)
                        .await?;
                }
            }
        }

        Ok(())
    }
}

pub(super) fn resolved_content_delivery(
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

fn should_load_declared_action_content(
    action: &ResourceActionDefinition,
    content: &ResourceContent,
    policy: &ResourceActionPolicy,
) -> bool {
    matches!(
        resolved_content_delivery(action, content.size(), policy),
        Some(ResourceActionContentDelivery::Inline)
    )
}
