use asset_core::domain::{Checksum, Resource, ResourceContent, ResourceDirectory};
use asset_core::port::{ResourceActionOutput, ResourceKindDefinition};
use asset_core::service::{ReadableResource, ResourceActions};
use asset_plugin_api::{
    PluginDiagnostic, PluginDiagnosticSeverity, ResourceActionAccess,
    ResourceActionContentDelivery, ResourceActionDefinition, ResourceActionExecutorKind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[allow(unused_imports)]
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

/// OpenAPI 中表示原始二进制请求或响应体的 schema。
#[derive(Debug, ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub(crate) struct BinaryContent(Vec<u8>);

/// 创建不包含对象内容的资源请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(example = json!({
    "name": "resources_not_blob",
    "kind": "core:unknown",
    "description": "A resource without blob content",
    "tags": ["demo", "document"]
}))]
pub(crate) struct CreateResourceRequest {
    /// 资源展示名。
    pub(crate) name: String,
    /// 可选资源类型。
    pub(crate) kind: Option<String>,
    /// 可选初始状态：`active` 或 `archived`。
    pub(crate) status: Option<String>,
    /// 资源所在逻辑目录；根目录为空字符串。
    #[schema(value_type = Option<String>)]
    pub(crate) directory: Option<ResourceDirectory>,
    /// 可选资源描述。
    pub(crate) description: Option<String>,
    /// 可选资源标签。
    pub(crate) tags: Option<Vec<String>>,
}

/// 创建逻辑目录请求。
#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateDirectoryRequest {
    /// 父目录路径，根目录为空字符串。
    #[serde(default)]
    #[schema(value_type = String)]
    pub(crate) parent_path: ResourceDirectory,
    /// 新目录名称，只允许单个路径段。
    pub(crate) name: String,
}

/// 资源列表查询参数。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListResourcesQuery {
    /// 页码，从 1 开始。
    pub(crate) page: Option<u32>,
    /// 每页数量。
    pub(crate) limit: Option<u32>,
    /// 可选资源类型过滤。
    pub(crate) kind: Option<String>,
    /// kind 过滤是否包含所有后代类型。
    pub(crate) include_descendants: Option<bool>,
    /// 可选标签过滤。
    pub(crate) tag: Option<String>,
    /// 可选名称模糊搜索关键字。
    pub(crate) q: Option<String>,
    /// 可选逻辑目录过滤；根目录为空字符串。
    #[param(value_type = Option<String>)]
    pub(crate) directory: Option<ResourceDirectory>,
    /// 是否包含软删除资源。
    pub(crate) include_deleted: Option<bool>,
}

/// 目录浏览查询参数。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListDirectoryQuery {
    /// 当前目录路径；根目录为空字符串。
    #[param(value_type = Option<String>)]
    pub(crate) path: Option<ResourceDirectory>,
    /// 资源页码，从 1 开始。
    pub(crate) page: Option<u32>,
    /// 每页资源数量。
    pub(crate) limit: Option<u32>,
    /// 可选资源类型过滤。
    pub(crate) kind: Option<String>,
    /// kind 过滤是否包含所有后代类型。
    pub(crate) include_descendants: Option<bool>,
    /// 可选标签过滤。
    pub(crate) tag: Option<String>,
    /// 可选名称模糊搜索关键字。
    pub(crate) q: Option<String>,
    /// 是否包含软删除资源。
    pub(crate) include_deleted: Option<bool>,
}

/// 更新资源请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(example = json!({
    "name": "renamed.txt",
    "kind": "core:unknown",
    "status": "archived",
    "description": "updated resource",
    "tags": ["demo", "updated"]
}))]
pub(crate) struct UpdateResourceRequest {
    /// 可选新资源展示名。
    pub(crate) name: Option<String>,
    /// 可选新资源类型。
    pub(crate) kind: Option<String>,
    /// 可选新状态：`active` 或 `archived`。
    pub(crate) status: Option<String>,
    /// 可选新逻辑目录；根目录为空字符串。
    #[schema(value_type = Option<String>)]
    pub(crate) directory: Option<ResourceDirectory>,
    /// 资源描述补丁：缺省表示不修改，`null` 表示清空。
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub(crate) description: Option<Option<String>>,
    /// 可选资源标签；提供时替换全部标签，空数组表示清空。
    pub(crate) tags: Option<Vec<String>>,
    /// 是否恢复软删除资源。
    pub(crate) restore: Option<bool>,
}

/// 上传内容并创建资源请求。
/// 流式上传内容并创建资源的 query 参数。
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(crate) struct UploadResourceContentStreamQuery {
    /// 资源文件名；与目录共同决定对象存储路径。
    pub(crate) name: String,
    /// 可选上传目录。
    #[param(value_type = Option<String>)]
    #[schema(value_type = Option<String>)]
    pub(crate) directory: Option<ResourceDirectory>,
    /// 可选资源类型。
    pub(crate) kind: Option<String>,
    /// 可选初始状态：`active` 或 `archived`。
    pub(crate) status: Option<String>,
    /// 可选资源描述。
    pub(crate) description: Option<String>,
    /// 可选 JSON 字符串形式的资源标签数组。
    pub(crate) tags_json: Option<String>,
}

/// 统一 HTTP 错误响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ErrorResponse {
    /// 错误说明。
    pub(crate) error: String,
    /// Stable machine-readable error code when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) code: Option<String>,
    /// Whether retrying the same operation may succeed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) retryable: Option<bool>,
    /// Optional structured diagnostic context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<serde_json::Value>,
    /// Additional structured diagnostics associated with the failure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<PluginDiagnosticResponse>,
}

/// 健康检查响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HealthResponse {
    /// 服务状态。
    pub(crate) status: String,
    pub(crate) database: HealthComponentResponse,
    pub(crate) blob_storage: HealthComponentResponse,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HealthComponentResponse {
    pub(crate) status: String,
}

/// 资源类型列表响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceKindsResponse {
    /// 当前后端支持的资源类型。
    pub(crate) items: Vec<ResourceKindResponse>,
}

/// 资源类型响应。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceKindResponse {
    /// 资源类型值。
    pub(crate) kind: String,
    /// 直接父类型；根类型为 null。
    pub(crate) parent: Option<String>,
    /// 从直接父类型到根类型的完整祖先链。
    pub(crate) ancestors: Vec<String>,
    /// 展示名称。
    pub(crate) label: String,
    /// 是否允许上传文件内容。
    pub(crate) supports_content: bool,
    /// 文件自动识别规则；为空时不会主动匹配，仅可作为手动选择或兜底。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detect: Option<ResourceContentMatcherResponse>,
    /// kind 支持的动作。
    pub(crate) actions: Vec<ResourceActionDefinitionResponse>,
    /// 定义来源：`builtin`、`config` 或 `plugin:<id>`。
    pub(crate) source: String,
}

impl ResourceKindResponse {
    pub(crate) fn from_definition(
        definition: &ResourceKindDefinition,
        registry: &dyn asset_core::port::ResourceKindRegistry,
        service: &asset_core::service::ResourceService,
    ) -> Self {
        Self {
            kind: definition.kind().as_str().to_string(),
            parent: definition.parent().map(|parent| parent.as_str().to_owned()),
            ancestors: registry
                .lineage(definition.kind())
                .into_iter()
                .skip(1)
                .map(|kind| kind.as_str().to_owned())
                .collect(),
            label: definition.label().to_string(),
            supports_content: definition.supports_content(),
            detect: (!definition.detect().is_empty()).then(|| ResourceContentMatcherResponse {
                mime_types: definition.detect().mime_types().to_vec(),
                extensions: definition.detect().extensions().to_vec(),
            }),
            actions: service
                .describe_kind_actions(definition.kind())
                .iter()
                .map(ResourceActionDefinitionResponse::from)
                .collect(),
            source: definition.source().to_string(),
        }
    }
}

/// 资源动作定义响应。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceActionDefinitionResponse {
    /// 动作 ID。
    pub(crate) id: String,
    /// 展示名称。
    pub(crate) label: String,
    /// 动作说明。
    pub(crate) description: Option<String>,
    /// 执行器声明。
    pub(crate) executor: ResourceActionExecutorResponse,
    /// 访问边界。
    pub(crate) access: String,
    /// 动作所需数据。
    pub(crate) requires: ResourceActionRequirementsResponse,
    /// 输出约定。
    pub(crate) output: ResourceActionOutputContractResponse,
    /// UI 展示提示。
    pub(crate) ui: ResourceActionUiResponse,
    /// 资源和内容匹配条件。
    pub(crate) applies_to: ResourceActionAppliesToResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceActionExecutorResponse {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceActionRequirementsResponse {
    pub(crate) content: bool,
    pub(crate) content_delivery: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceActionOutputContractResponse {
    pub(crate) view: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceActionUiResponse {
    pub(crate) group: Option<String>,
    pub(crate) order: Option<i32>,
    pub(crate) locations: Vec<String>,
}

/// 内容匹配条件。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceContentMatcherResponse {
    /// 匹配的 MIME 类型，支持 `image/*` 这类通配前缀。
    pub(crate) mime_types: Vec<String>,
    /// 匹配的文件扩展名。
    pub(crate) extensions: Vec<String>,
}

/// 资源动作适用范围。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceActionAppliesToResponse {
    pub(crate) kinds: Vec<String>,
    pub(crate) mime_types: Vec<String>,
    pub(crate) extensions: Vec<String>,
}

impl From<&ResourceActionDefinition> for ResourceActionDefinitionResponse {
    fn from(action: &ResourceActionDefinition) -> Self {
        Self {
            id: action.id().as_str().to_string(),
            label: action.label().to_string(),
            description: action.description().map(str::to_string),
            executor: ResourceActionExecutorResponse {
                kind: action_executor_text(action.executor()).to_string(),
                handler: action.handler().map(str::to_string),
            },
            access: action_access_text(action.access()).to_string(),
            requires: ResourceActionRequirementsResponse {
                content: action.requirements().content,
                content_delivery: content_delivery_text(action.requirements().content_delivery)
                    .to_string(),
            },
            output: ResourceActionOutputContractResponse {
                view: action.output().view.clone(),
            },
            ui: ResourceActionUiResponse {
                group: action.ui().group.clone(),
                order: action.ui().order,
                locations: action.ui().locations.clone(),
            },
            applies_to: ResourceActionAppliesToResponse {
                kinds: action.applies_to().kinds().to_vec(),
                mime_types: action.content_matcher().mime_types().to_vec(),
                extensions: action.content_matcher().extensions().to_vec(),
            },
        }
    }
}

fn action_executor_text(executor: ResourceActionExecutorKind) -> &'static str {
    match executor {
        ResourceActionExecutorKind::Builtin => "builtin",
        ResourceActionExecutorKind::Plugin => "plugin",
    }
}

fn action_access_text(access: ResourceActionAccess) -> &'static str {
    match access {
        ResourceActionAccess::ReadOnly => "read_only",
        ResourceActionAccess::ReadWrite => "read_write",
    }
}

fn content_delivery_text(delivery: ResourceActionContentDelivery) -> &'static str {
    match delivery {
        ResourceActionContentDelivery::Auto => "auto",
        ResourceActionContentDelivery::Inline => "inline",
        ResourceActionContentDelivery::Reference => "reference",
    }
}

impl HealthResponse {
    pub(crate) fn new(database_ready: bool, blob_storage_ready: bool) -> Self {
        Self {
            status: if database_ready && blob_storage_ready {
                "ready"
            } else {
                "unavailable"
            }
            .to_string(),
            database: HealthComponentResponse {
                status: component_status(database_ready),
            },
            blob_storage: HealthComponentResponse {
                status: component_status(blob_storage_ready),
            },
        }
    }
}

fn component_status(ready: bool) -> String {
    if ready { "ready" } else { "unavailable" }.to_string()
}

/// 资源响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceResponse {
    /// 资源唯一标识。
    pub(crate) id: String,
    /// 资源展示名。
    pub(crate) name: String,
    /// 资源所在逻辑目录，根目录为空字符串。
    #[schema(value_type = String)]
    pub(crate) directory: ResourceDirectory,
    /// 资源类型。
    pub(crate) kind: String,
    /// 资源生命周期状态。
    pub(crate) status: String,
    /// 可选资源描述。
    pub(crate) description: Option<String>,
    /// 资源标签。
    pub(crate) tags: Vec<String>,
    /// 资源内容引用。
    pub(crate) content: Option<ResourceContentResponse>,
    /// 当前资源允许的操作。
    pub(crate) actions: ResourceActionsResponse,
    /// 资源创建时间，RFC3339 格式。
    pub(crate) created_at: String,
    /// 资源最后更新时间，RFC3339 格式。
    pub(crate) updated_at: String,
    /// 软删除时间，RFC3339 格式；为空表示未删除。
    pub(crate) deleted_at: Option<String>,
}

/// 资源允许的操作集合。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceActionsResponse {
    /// 当前资源允许的动作。
    pub(crate) available_actions: Vec<ResourceActionDefinitionResponse>,
}

/// 资源分页响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourcePageResponse {
    /// 当前页资源。
    pub(crate) items: Vec<ResourceResponse>,
    /// 符合条件的总记录数。
    pub(crate) total: u64,
    /// 当前页码，从 1 开始。
    pub(crate) page: u32,
    /// 每页数量。
    pub(crate) limit: u32,
}

/// 逻辑目录响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceDirectoryResponse {
    /// 目录完整路径。
    pub(crate) path: String,
    /// 父目录路径。
    pub(crate) parent_path: String,
    /// 当前目录名。
    pub(crate) name: String,
}

/// 目录浏览响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DirectoryListingResponse {
    /// 当前目录路径。
    #[schema(value_type = String)]
    pub(crate) path: ResourceDirectory,
    /// 直接子目录。
    pub(crate) folders: Vec<ResourceDirectoryResponse>,
    /// 当前目录下的资源分页。
    pub(crate) resources: ResourcePageResponse,
}

/// 在线阅读响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceReadResponse {
    /// 资源唯一标识。
    pub(crate) id: String,
    /// 资源展示名。
    pub(crate) name: String,
    /// 资源类型。
    pub(crate) kind: String,
    /// 插件返回的 View。
    pub(crate) view: Value,
}

/// 执行资源动作请求。
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "input": {
        "mode": "default"
    }
}))]
pub(crate) struct ExecuteResourceActionRequest {
    /// 传递给插件 action handler 的 JSON 输入。
    #[serde(default)]
    pub(crate) input: Value,
}

/// 执行资源动作响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceActionOutputResponse {
    /// 资源唯一标识。
    pub(crate) resource_id: String,
    /// 动作 ID。
    pub(crate) action: String,
    /// 插件返回的 View。
    pub(crate) view: Value,
    /// 插件返回的非致命诊断信息。
    pub(crate) diagnostics: Vec<PluginDiagnosticResponse>,
}

/// 插件或宿主产生的结构化诊断信息。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PluginDiagnosticResponse {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) severity: String,
    pub(crate) retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<Value>,
}

impl From<&PluginDiagnostic> for PluginDiagnosticResponse {
    fn from(diagnostic: &PluginDiagnostic) -> Self {
        Self {
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            severity: match diagnostic.severity {
                PluginDiagnosticSeverity::Info => "info",
                PluginDiagnosticSeverity::Warning => "warning",
                PluginDiagnosticSeverity::Error => "error",
            }
            .to_string(),
            retryable: diagnostic.retryable,
            details: diagnostic.details.clone(),
        }
    }
}

impl From<&ResourceActionOutput> for ResourceActionOutputResponse {
    fn from(output: &ResourceActionOutput) -> Self {
        Self {
            resource_id: output.resource_id().to_string(),
            action: output.action().as_str().to_string(),
            view: serde_json::to_value(&output.output().view)
                .expect("plugin view should serialize to JSON"),
            diagnostics: output
                .output()
                .diagnostics
                .iter()
                .map(PluginDiagnosticResponse::from)
                .collect(),
        }
    }
}

impl From<&ReadableResource> for ResourceReadResponse {
    fn from(resource: &ReadableResource) -> Self {
        Self {
            id: resource.id().to_string(),
            name: resource.name().to_string(),
            kind: resource.kind().as_str().to_string(),
            view: serde_json::to_value(resource.view())
                .expect("plugin view should serialize to JSON"),
        }
    }
}

impl ResourceResponse {
    pub(crate) fn new(resource: &Resource, actions: ResourceActions) -> Self {
        Self {
            id: resource.id().to_string(),
            name: resource.name().to_string(),
            directory: resource.directory().clone(),
            kind: resource.kind().as_str().to_string(),
            status: resource.status().as_str().to_string(),
            description: resource.description().map(str::to_string),
            tags: resource
                .tags()
                .iter()
                .map(|tag| tag.as_str().to_owned())
                .collect(),
            content: resource.content().map(ResourceContentResponse::from),
            actions: ResourceActionsResponse::from(actions),
            created_at: resource.created_at().to_rfc3339(),
            updated_at: resource.updated_at().to_rfc3339(),
            deleted_at: resource.deleted_at().map(|value| value.to_rfc3339()),
        }
    }
}

fn deserialize_optional_field<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

impl From<ResourceActions> for ResourceActionsResponse {
    fn from(actions: ResourceActions) -> Self {
        Self {
            available_actions: actions
                .available_actions()
                .iter()
                .map(ResourceActionDefinitionResponse::from)
                .collect(),
        }
    }
}

/// 资源内容引用响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceContentResponse {
    /// 内容字节大小。
    pub(crate) size: u64,
    /// 内容 MIME 类型。
    pub(crate) mime_type: Option<String>,
    /// 服务端根据内容本体计算得到的校验和。
    pub(crate) checksum: ChecksumResponse,
}

impl From<&ResourceContent> for ResourceContentResponse {
    fn from(content: &ResourceContent) -> Self {
        Self {
            size: content.size(),
            mime_type: content.mime_type().map(str::to_string),
            checksum: ChecksumResponse::from(content.checksum()),
        }
    }
}

/// 内容校验和响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ChecksumResponse {
    /// 校验和算法类型。
    pub(crate) kind: String,
    /// 校验和值。
    pub(crate) value: String,
}

impl From<&Checksum> for ChecksumResponse {
    fn from(checksum: &Checksum) -> Self {
        Self {
            kind: checksum.kind().as_str().to_string(),
            value: checksum.value().to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
