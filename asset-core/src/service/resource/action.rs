//! 资源动作服务。
//!
//! 本模块负责把资源、kind/action 声明和动作执行器连接起来：解析可用动作、执行声明动作，并应用动作返回的写入效果。

use super::content::{build_content, calculate_checksum};
use super::{ExecuteResourceAction, ResourceActions, ResourceService};
use crate::CoreError;
#[cfg(test)]
use crate::domain::ResourceId;
use crate::domain::{Resource, ResourceContent, StorageKey};
use crate::port::{
    BlobStorage, RESERVED_BLOB_STORAGE_PREFIX, ResourceActionOutput, ResourceActionRequest,
};
use asset_plugin_api::{
    PluginActionEffect, PluginExecutionPolicy, ResourceAction, ResourceActionAccess,
    ResourceActionContentDelivery, ResourceActionDefinition,
};
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

        let available_actions = self
            .service
            .actions_for_resource_kind(resource.kind())
            .into_iter()
            .filter(|action| resource.content().is_some() || !action.requirements().content)
            .filter(|action| self.service.action_matches_resource(action, resource))
            .collect::<Vec<_>>();

        Ok(ResourceActions::new(available_actions))
    }

    /// 执行资源类型声明的插件动作。
    ///
    /// 核心负责资源存在性、删除状态、kind/action 声明、访问边界和对象内容加载；具体 wasm
    /// 运行时由 `ResourceActionExecutor` 端口承接。
    pub(super) async fn execute_resource_action_snapshot(
        &self,
        resource: Resource,
        command: ExecuteResourceAction,
    ) -> Result<ResourceActionOutput, CoreError> {
        self.execute_declared_resource_action_snapshot(resource, command.action, command.input)
            .await
    }

    pub(super) async fn execute_declared_resource_action_snapshot(
        &self,
        mut resource: Resource,
        action_id: ResourceAction,
        input: serde_json::Value,
    ) -> Result<ResourceActionOutput, CoreError> {
        // 1. Resolve the action from the resource kind/global action registry before touching
        //    content or plugin runtime state.
        let action = self.resolve_declared_resource_action(&resource, &action_id)?;

        // 2. Load content only when the action contract says the executor should receive it.
        let content = self
            .load_declared_resource_action_content(&resource, &action)
            .await?;

        // 3. Dispatch the request through the configured action executor.
        let access = action.access();
        let output = self
            .execute_resource_action_request(&resource, action_id, &action, input, content)
            .await?;
        self.validate_action_output(&action, &output)?;

        // 4. Apply write effects after the executor returns, guarded by the action access boundary.
        self.apply_action_effects(&mut resource, &output, access)
            .await?;

        Ok(output)
    }

    pub(super) fn resolve_declared_resource_action(
        &self,
        resource: &Resource,
        action_id: &ResourceAction,
    ) -> Result<ResourceActionDefinition, CoreError> {
        self.service.require_kind_definition(resource.kind())?;
        if resource.is_deleted() {
            return Err(CoreError::configuration(format!(
                "deleted resource `{}` cannot execute actions",
                resource.id()
            )));
        }
        let declared_actions = self.service.actions_for_resource_kind(resource.kind());
        declared_actions
            .into_iter()
            .find(|action| {
                action.id().as_str() == action_id.as_str()
                    && (resource.content().is_some() || !action.requirements().content)
                    && self.service.action_matches_resource(action, resource)
            })
            .ok_or_else(|| {
                CoreError::configuration(format!(
                    "resource kind `{}` does not support action `{}`",
                    resource.kind(),
                    action_id
                ))
            })
    }

    fn validate_action_output(
        &self,
        action: &ResourceActionDefinition,
        output: &ResourceActionOutput,
    ) -> Result<(), CoreError> {
        let actual = output.output().view.kind();
        if !action
            .output()
            .view
            .iter()
            .any(|declared| declared == actual)
        {
            return Err(CoreError::configuration(format!(
                "action `{}` returned undeclared view `{actual}`",
                action.id()
            )));
        }
        let replacements = output
            .output()
            .effects
            .iter()
            .filter(|effect| matches!(effect, PluginActionEffect::ReplaceContent(_)))
            .count();
        if replacements > 1 {
            return Err(CoreError::configuration(format!(
                "action `{}` returned more than one replace_content effect",
                action.id()
            )));
        }
        Ok(())
    }

    async fn load_declared_resource_action_content(
        &self,
        resource: &Resource,
        action: &ResourceActionDefinition,
    ) -> Result<Option<Bytes>, CoreError> {
        let Some(content_ref) = resource.content() else {
            return Ok(None);
        };
        if !should_load_declared_action_content(
            action,
            content_ref,
            &self.service.plugin_execution_policy,
        ) {
            return Ok(None);
        }
        let max_content_bytes = self.service.plugin_execution_policy.max_content_bytes();
        if content_ref.size() > max_content_bytes {
            return Err(CoreError::configuration(format!(
                "plugin actions are limited to {max_content_bytes} bytes of resource content"
            )));
        }

        self.service.blob_storage.get(&resource.storage_key()).await
    }

    async fn execute_resource_action_request(
        &self,
        resource: &Resource,
        action_id: ResourceAction,
        action: &ResourceActionDefinition,
        input: serde_json::Value,
        content: Option<Bytes>,
    ) -> Result<ResourceActionOutput, CoreError> {
        let Some(ports) = &self.service.action_ports else {
            return Err(CoreError::configuration(
                "resource action executor is not configured",
            ));
        };
        let request = ResourceActionRequest::new(
            resource.clone(),
            action_id,
            action.handler(),
            action.access(),
            resource
                .content()
                .and_then(|content| {
                    resolved_content_delivery(
                        action,
                        content.size(),
                        &self.service.plugin_execution_policy,
                    )
                })
                .unwrap_or(ResourceActionContentDelivery::Auto),
            input,
            content,
        );

        ports.executor.execute(request).await
    }

    async fn apply_action_effects(
        &self,
        resource: &mut Resource,
        output: &ResourceActionOutput,
        access: ResourceActionAccess,
    ) -> Result<(), CoreError> {
        if output.output().effects.is_empty() {
            return Ok(());
        }
        if !matches!(access, ResourceActionAccess::ReadWrite) {
            return Err(CoreError::configuration(format!(
                "action `{}` returned effects without write access",
                output.action()
            )));
        }

        for effect in &output.output().effects {
            match effect {
                PluginActionEffect::ReplaceContent(effect) => {
                    let Some(current_content) = resource.content().cloned() else {
                        return Err(CoreError::configuration(format!(
                            "action `{}` cannot replace missing resource content",
                            output.action()
                        )));
                    };
                    let data = BASE64_STANDARD
                        .decode(effect.data.as_bytes())
                        .map(Bytes::from)
                        .map_err(|error| {
                            CoreError::configuration(format!(
                                "action `{}` returned invalid replace_content base64: {error}",
                                output.action()
                            ))
                        })?;
                    let checksum = calculate_checksum(data.as_ref())?;
                    let target_key = resource.storage_key();
                    let replacement_key = action_scratch_content_key("action-replacements")?;
                    let backup_key = action_scratch_content_key("action-backups")?;
                    let content = build_content(
                        data.len() as u64,
                        effect
                            .mime_type
                            .clone()
                            .or_else(|| current_content.mime_type().map(str::to_string)),
                        checksum,
                        None,
                    )?;

                    let expected_updated_at = resource.updated_at();
                    let previous = self.service.blob_storage.get(&target_key).await?;
                    self.service
                        .blob_storage
                        .put(&replacement_key, data.clone())
                        .await?;
                    if let Some(previous) = previous.clone() {
                        self.service.blob_storage.put(&backup_key, previous).await?;
                    }
                    self.service.blob_storage.put(&target_key, data).await?;
                    resource.attach_content(content)?;
                    let saved = self
                        .service
                        .repository
                        .save_if_unchanged(resource, expected_updated_at)
                        .await;
                    match saved {
                        // Old versions stay immutable and can be collected after repository-wide
                        // reachability analysis. Deleting here could race a shared reference.
                        Ok(true) => {
                            let _ = self.service.blob_storage.delete(&replacement_key).await;
                            let _ = self.service.blob_storage.delete(&backup_key).await;
                        }
                        Ok(false) => {
                            restore_replaced_content(
                                self.service.blob_storage.as_ref(),
                                &target_key,
                                &backup_key,
                                previous,
                            )
                            .await;
                            let _ = self.service.blob_storage.delete(&replacement_key).await;
                            let _ = self.service.blob_storage.delete(&backup_key).await;
                            return Err(CoreError::conflict(format!(
                                "resource `{}` changed while action `{}` was running",
                                resource.id(),
                                output.action()
                            )));
                        }
                        Err(error) => {
                            restore_replaced_content(
                                self.service.blob_storage.as_ref(),
                                &target_key,
                                &backup_key,
                                previous,
                            )
                            .await;
                            let _ = self.service.blob_storage.delete(&replacement_key).await;
                            let _ = self.service.blob_storage.delete(&backup_key).await;
                            return Err(error);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

pub(super) fn resolved_content_delivery(
    action: &ResourceActionDefinition,
    size: u64,
    policy: &PluginExecutionPolicy,
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
    policy: &PluginExecutionPolicy,
) -> bool {
    matches!(
        resolved_content_delivery(action, content.size(), policy),
        Some(ResourceActionContentDelivery::Inline)
    )
}

fn action_scratch_content_key(suffix: &str) -> Result<StorageKey, CoreError> {
    Ok(StorageKey::new(format!(
        "{}/action-effects/{suffix}/{}",
        RESERVED_BLOB_STORAGE_PREFIX,
        uuid::Uuid::now_v7()
    ))?)
}

async fn restore_replaced_content(
    blob_storage: &dyn BlobStorage,
    current_key: &StorageKey,
    backup_key: &StorageKey,
    previous: Option<Bytes>,
) {
    if let Some(previous) = previous {
        let _ = blob_storage.put(current_key, previous).await;
    } else {
        let _ = blob_storage.delete(current_key).await;
    }
    let _ = blob_storage.delete(backup_key).await;
}
