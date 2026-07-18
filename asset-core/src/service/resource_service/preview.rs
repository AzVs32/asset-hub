//! 资源预览服务。
//!
//! 本模块负责面向展示的读取用例：在线阅读、预览、缩略图和预览流。具体解析仍由 action/plugin 体系提供。

use super::*;

/// 资源预览服务。
///
/// 预览服务依赖动作服务执行 read/preview/thumbnail 声明动作，再把插件 view 转换为调用方需要的内容结构。
pub(super) struct ResourcePreviewService<'a> {
    service: &'a ResourceService,
}

impl<'a> ResourcePreviewService<'a> {
    /// 创建资源预览服务。
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    /// 返回同一资源上下文下的动作服务。
    fn actions(&self) -> ResourceActionService<'a> {
        ResourceActionService::new(self.service)
    }

    /// 读取资源的可阅读 View。
    ///
    /// 该 usecase 统一负责 `read` action 校验、对象内容读取和插件调度，供 HTTP、CLI、
    /// TUI 等应用入口复用。具体格式解析由插件负责。
    ///
    /// 找不到资源、资源已删除或没有内容时返回 `Ok(None)`。资源类型不支持阅读，或内容格式
    /// 没有插件 handler 时返回 `Err(CoreError::Configuration { .. })`。
    #[cfg(test)]
    pub(crate) async fn read_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ReadableResource>, CoreError> {
        let Some(resource) = self.service.commands().find_resource(id).await? else {
            return Ok(None);
        };

        self.read_resource_snapshot(resource).await.map(Some)
    }

    pub(crate) async fn read_resource_snapshot(
        &self,
        resource: Resource,
    ) -> Result<ReadableResource, CoreError> {
        let output = self
            .actions()
            .execute_declared_resource_action_snapshot(
                resource.clone(),
                ResourceAction::READ.into(),
                serde_json::Value::Null,
            )
            .await?;

        Ok(ReadableResource::new(
            resource.id(),
            resource.name().to_string(),
            resource.kind().clone(),
            output.output().view.clone(),
        ))
    }

    /// 读取资源预览内容。
    #[cfg(test)]
    pub(super) async fn preview_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourcePreview>, CoreError> {
        let Some(resource) = self.service.commands().find_resource(id).await? else {
            return Ok(None);
        };

        self.preview_resource_snapshot(resource).await.map(Some)
    }

    #[cfg(test)]
    async fn preview_resource_snapshot(
        &self,
        resource: Resource,
    ) -> Result<ResourcePreview, CoreError> {
        let output = self
            .actions()
            .execute_declared_resource_action_snapshot(
                resource.clone(),
                ResourceAction::PREVIEW.into(),
                serde_json::Value::Null,
            )
            .await?;
        let (content_type, content) = self
            .media_view_content(&resource, ResourceAction::PREVIEW, &output.output().view)
            .await?;

        Ok(ResourcePreview::new(content_type, content))
    }

    /// 返回资源预览内容流。
    pub(crate) async fn preview_resource_stream_snapshot(
        &self,
        resource: &Resource,
    ) -> Result<ResourcePreviewStream, CoreError> {
        self.service.require_kind_definition(resource.kind())?;
        let declared_actions = self.service.actions_for_resource_kind(resource.kind());
        let Some(action) = declared_actions.iter().find(|action| {
            action.id().as_str() == ResourceAction::PREVIEW
                && self.service.action_matches_resource(action, resource)
        }) else {
            return Err(CoreError::configuration(format!(
                "resource kind `{}` does not support action `preview`",
                resource.kind()
            )));
        };
        if action.executor() != ResourceActionExecutorKind::Builtin {
            return Err(CoreError::configuration(
                "plugin preview actions must be executed through the action endpoint",
            ));
        }
        let Some(content_ref) = resource.content() else {
            return Err(CoreError::not_found(
                "resource content",
                resource.id().to_string(),
            ));
        };
        let Some(content) = self
            .service
            .blob_storage
            .get_stream(&resource.storage_key())
            .await?
        else {
            return Err(CoreError::not_found(
                "resource content",
                resource.id().to_string(),
            ));
        };

        Ok(ResourcePreviewStream::new(
            content_type_for_media(content_ref),
            Some(content_ref.size()),
            content,
        ))
    }

    /// 读取资源缩略图内容。
    #[cfg(test)]
    pub(crate) async fn thumbnail_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourceThumbnail>, CoreError> {
        let Some(resource) = self.service.commands().find_resource(id).await? else {
            return Ok(None);
        };

        self.thumbnail_resource_snapshot(resource).await.map(Some)
    }

    pub(crate) async fn thumbnail_resource_snapshot(
        &self,
        resource: Resource,
    ) -> Result<ResourceThumbnail, CoreError> {
        let output = self
            .actions()
            .execute_declared_resource_action_snapshot(
                resource.clone(),
                ResourceAction::THUMBNAIL.into(),
                serde_json::Value::Null,
            )
            .await?;
        let (content_type, content) = self
            .media_view_content(&resource, ResourceAction::THUMBNAIL, &output.output().view)
            .await?;

        Ok(ResourceThumbnail::new(content_type, content))
    }

    async fn media_view_content(
        &self,
        resource: &Resource,
        action: &str,
        view: &PluginView,
    ) -> Result<(String, Bytes), CoreError> {
        let PluginView::Media(media) = view else {
            return Err(CoreError::configuration(format!(
                "resource action `{action}` must return a media view"
            )));
        };

        match media.encoding {
            PluginMediaEncoding::Base64 => decode_media_view(action, view),
            PluginMediaEncoding::Url => {
                let Some(content_ref) = resource.content() else {
                    return Err(CoreError::not_found(
                        "resource content",
                        resource.id().to_string(),
                    ));
                };
                let Some(content) = self
                    .service
                    .blob_storage
                    .get(&resource.storage_key())
                    .await?
                else {
                    return Err(CoreError::not_found(
                        "resource content",
                        resource.id().to_string(),
                    ));
                };
                let content_type = if media.mime_type.trim().is_empty() {
                    content_type_for_media(content_ref)
                } else {
                    media.mime_type.clone()
                };
                Ok((content_type, content))
            }
        }
    }
}
