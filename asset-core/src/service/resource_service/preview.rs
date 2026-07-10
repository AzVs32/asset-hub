//! 资源预览服务。
//!
//! 本模块负责面向展示的读取用例：在线阅读、预览、缩略图和预览流。具体解析仍由 action/plugin 体系提供。

use super::*;

/// 资源预览服务。
///
/// 预览服务依赖动作服务执行 read/preview/thumbnail 声明动作，再把插件 view 转换为调用方需要的内容结构。
pub struct ResourcePreviewService<'a> {
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
    pub async fn read_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ReadableResource>, CoreError> {
        let Some(output) = self
            .actions()
            .execute_declared_resource_action(
                id,
                crate::port::ResourceAction::READ.into(),
                serde_json::Value::Null,
            )
            .await?
        else {
            return Ok(None);
        };
        let Some(resource) = self.service.find_resource(id).await? else {
            return Ok(None);
        };

        Ok(Some(ReadableResource::new(
            resource.id(),
            resource.name().to_string(),
            resource.kind().clone(),
            output.output().view.clone(),
        )))
    }

    /// 读取资源预览内容。
    pub async fn preview_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourcePreview>, CoreError> {
        let Some(output) = self
            .actions()
            .execute_declared_resource_action(
                id,
                crate::port::ResourceAction::PREVIEW.into(),
                serde_json::Value::Null,
            )
            .await?
        else {
            return Ok(None);
        };
        let (content_type, content) = self
            .media_view_content(
                id,
                crate::port::ResourceAction::PREVIEW,
                &output.output().view,
            )
            .await?;

        Ok(Some(ResourcePreview::new(content_type, content)))
    }

    /// 返回资源预览内容流。
    pub async fn preview_resource_stream(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourcePreviewStream>, CoreError> {
        let Some(resource) = self.service.find_resource(id).await? else {
            return Ok(None);
        };
        let definition = self.service.require_kind_definition(resource.kind())?;
        let declared_actions = self.service.actions_for_resource(&resource, &definition);
        let content_ref = resource.content();
        let Some(action) = declared_actions.iter().find(|action| {
            action.id().as_str() == crate::port::ResourceAction::PREVIEW
                && action.matches_resource(
                    resource.kind().as_str(),
                    content_ref.and_then(|content| content.mime_type()),
                    content_ref.map(|content| content.key().as_str()),
                )
        }) else {
            return Err(CoreError::configuration(format!(
                "resource kind `{}` does not support action `preview`",
                resource.kind()
            )));
        };
        if action.executor() != crate::port::ResourceActionExecutorKind::Builtin {
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
            .get_stream(content_ref.key())
            .await?
        else {
            return Err(CoreError::not_found(
                "resource content",
                resource.id().to_string(),
            ));
        };

        Ok(Some(ResourcePreviewStream::new(
            content_type_for_media(content_ref),
            Some(content_ref.size()),
            content,
        )))
    }

    /// 读取资源缩略图内容。
    pub async fn thumbnail_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourceThumbnail>, CoreError> {
        let Some(output) = self
            .actions()
            .execute_declared_resource_action(
                id,
                crate::port::ResourceAction::THUMBNAIL.into(),
                serde_json::Value::Null,
            )
            .await?
        else {
            return Ok(None);
        };
        let (content_type, content) = self
            .media_view_content(
                id,
                crate::port::ResourceAction::THUMBNAIL,
                &output.output().view,
            )
            .await?;

        Ok(Some(ResourceThumbnail::new(content_type, content)))
    }

    async fn media_view_content(
        &self,
        id: &ResourceId,
        action: &str,
        view: &PluginView,
    ) -> Result<(String, Bytes), CoreError> {
        let PluginView::Media(media) = view else {
            return Err(CoreError::configuration(format!(
                "resource action `{action}` must return a media view"
            )));
        };

        match media.encoding {
            PluginContentEncoding::Base64 => decode_media_view(action, view),
            PluginContentEncoding::Url => {
                let Some(resource) = self.service.find_resource(id).await? else {
                    return Err(CoreError::not_found("resource", id.to_string()));
                };
                let Some(content_ref) = resource.content() else {
                    return Err(CoreError::not_found("resource content", id.to_string()));
                };
                let Some(content) = self.service.blob_storage.get(content_ref.key()).await? else {
                    return Err(CoreError::not_found("resource content", id.to_string()));
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
