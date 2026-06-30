use asset_core::ResourceError;
use asset_core::domain::{
    Checksum, ChecksumKind, KindMetadata, Resource, ResourceContent, ResourceMetadata,
    ResourceStatus,
};
use asset_core::port::{
    ResourceActionAccess, ResourceActionDefinition, ResourceActionOutput, ResourceKindDefinition,
};
use asset_core::service::{ReadableResource, ResourceActions};
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

/// 创建纯元数据资源请求。
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "name": "resources_not_blob",
    "kind": "core:unknown",
    "metadata": {
        "summary": {
            "description": "A metadata-only resource",
            "tags": ["demo", "document"]
        },
        "kind": {
            "schema_id": "test:metadata@1",
            "data": {
                "source": "swagger"
            }
        }
    }
}))]
pub(crate) struct CreateResourceRequest {
    /// 资源展示名。
    pub(crate) name: String,
    /// 可选资源类型。
    pub(crate) kind: Option<String>,
    /// 可选初始状态：`active` 或 `archived`。
    pub(crate) status: Option<String>,
    /// 可选资源元数据。
    pub(crate) metadata: Option<ResourceMetadataRequest>,
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
    /// 可选标签过滤。
    pub(crate) tag: Option<String>,
    /// 可选名称模糊搜索关键字。
    pub(crate) q: Option<String>,
    /// 是否包含软删除资源。
    pub(crate) include_deleted: Option<bool>,
}

/// 更新资源请求。
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "name": "renamed.txt",
    "kind": "core:unknown",
    "status": "archived",
    "metadata": {
        "summary": {
            "description": "updated resource",
            "tags": ["demo", "updated"]
        },
        "kind": {
            "schema_id": "test:metadata@1",
            "data": {
                "source": "patch"
            }
        }
    }
}))]
pub(crate) struct UpdateResourceRequest {
    /// 可选新资源展示名。
    pub(crate) name: Option<String>,
    /// 可选新资源类型。
    pub(crate) kind: Option<String>,
    /// 可选新状态：`active` 或 `archived`。
    pub(crate) status: Option<String>,
    /// 可选新资源元数据；会整体替换旧元数据。
    pub(crate) metadata: Option<ResourceMetadataRequest>,
    /// 是否恢复软删除资源。
    pub(crate) restore: Option<bool>,
}

/// 上传内容并创建资源请求。
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "name": "hello.txt",
    "kind": "core:unknown",
    "storage_key": "examples/hello.txt",
    "data_base64": "aGVsbG8sIGFzc2V0LWh1YiE=",
    "metadata": {
        "summary": {
            "description": "A small text file",
            "tags": ["demo", "text"]
        },
        "kind": {
            "schema_id": "test:metadata@1",
            "data": {
                "source": "swagger"
            }
        }
    },
    "mime_type": "text/plain",
    "original_filename": "hello.txt"
}))]
pub(crate) struct UploadResourceContentRequest {
    /// 资源展示名。
    pub(crate) name: String,
    /// 对象存储键。
    pub(crate) storage_key: String,
    /// base64 编码后的内容字节。
    pub(crate) data_base64: String,
    /// 可选资源类型。
    pub(crate) kind: Option<String>,
    /// 可选初始状态：`active` 或 `archived`。
    pub(crate) status: Option<String>,
    /// 可选资源元数据。
    pub(crate) metadata: Option<ResourceMetadataRequest>,
    /// 可选 MIME 类型。
    pub(crate) mime_type: Option<String>,
    /// 可选原始文件名。
    pub(crate) original_filename: Option<String>,
    /// 可选 SHA-256 校验和。
    pub(crate) sha256: Option<String>,
}

/// 流式上传内容并创建资源的 query 参数。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct UploadResourceContentStreamQuery {
    /// 资源展示名。
    pub(crate) name: String,
    /// 对象存储键。
    pub(crate) storage_key: String,
    /// 可选资源类型。
    pub(crate) kind: Option<String>,
    /// 可选初始状态：`active` 或 `archived`。
    pub(crate) status: Option<String>,
    /// 可选 JSON 字符串形式的资源元数据，结构与 `ResourceMetadataRequest` 一致。
    pub(crate) metadata_json: Option<String>,
    /// 可选原始文件名。
    pub(crate) original_filename: Option<String>,
    /// 可选 SHA-256 校验和。
    pub(crate) sha256: Option<String>,
}

/// 统一 HTTP 错误响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ErrorResponse {
    /// 错误说明。
    pub(crate) error: String,
}

/// 健康检查响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HealthResponse {
    /// 服务状态。
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
    /// 展示名称。
    pub(crate) label: String,
    /// 默认 kind metadata schema id。
    pub(crate) schema_id: Option<String>,
    /// kind metadata JSON schema。
    pub(crate) metadata_schema: Option<Value>,
    /// 是否允许上传文件内容。
    pub(crate) supports_content: bool,
    /// kind 支持的动作。
    pub(crate) actions: Vec<ResourceActionDefinitionResponse>,
    /// 定义来源：`builtin`、`config` 或 `plugin:<id>`。
    pub(crate) source: String,
}

impl From<&ResourceKindDefinition> for ResourceKindResponse {
    fn from(definition: &ResourceKindDefinition) -> Self {
        Self {
            kind: definition.kind().as_str().to_string(),
            label: definition.label().to_string(),
            schema_id: definition.schema_id().map(str::to_string),
            metadata_schema: definition.metadata_schema().cloned(),
            supports_content: definition.supports_content(),
            actions: definition
                .actions()
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
    /// 访问边界。
    pub(crate) access: String,
}

impl From<&ResourceActionDefinition> for ResourceActionDefinitionResponse {
    fn from(action: &ResourceActionDefinition) -> Self {
        Self {
            id: action.id().as_str().to_string(),
            label: action.label().to_string(),
            access: action_access_text(action.access()).to_string(),
        }
    }
}

fn action_access_text(access: ResourceActionAccess) -> &'static str {
    match access {
        ResourceActionAccess::ReadOnly => "read_only",
        ResourceActionAccess::ReadWrite => "read_write",
    }
}

impl HealthResponse {
    /// 构造正常服务状态响应。
    pub(crate) fn ok() -> Self {
        Self {
            status: "ok".to_string(),
        }
    }
}

/// 创建或上传资源时可传入的资源元数据。
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[schema(example = json!({
    "summary": {
        "description": "Human readable resource description",
        "tags": ["demo", "asset"]
    },
    "kind": {
        "schema_id": "test:metadata@1",
        "data": {
            "source": "swagger"
        }
    }
}))]
pub(crate) struct ResourceMetadataRequest {
    /// 核心摘要元数据。
    pub(crate) summary: Option<ResourceSummaryMetadataRequest>,
    /// kind/plugin 专属元数据。
    pub(crate) kind: Option<KindMetadataRequest>,
}

/// 创建或上传资源时可传入的核心摘要元数据。
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub(crate) struct ResourceSummaryMetadataRequest {
    /// 资源描述。
    pub(crate) description: Option<String>,
    /// 资源标签。
    pub(crate) tags: Option<Vec<String>>,
}

/// 创建或上传资源时可传入的 kind/plugin 专属元数据。
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct KindMetadataRequest {
    /// 插件 schema 标识，例如 `mindustry:mod@1`。
    pub(crate) schema_id: String,
    /// kind/plugin 专属数据，必须是 JSON object。
    pub(crate) data: Value,
}

impl ResourceMetadataRequest {
    /// 转换为领域元数据并执行领域校验。
    pub(crate) fn into_domain(self) -> Result<ResourceMetadata, ResourceError> {
        let mut builder = ResourceMetadata::builder();

        if let Some(summary) = self.summary {
            if let Some(description) = summary.description {
                builder = builder.with_description(description);
            }

            if let Some(tags) = summary.tags {
                builder = builder.with_tags(tags);
            }
        }

        if let Some(kind) = self.kind {
            builder = builder.with_kind_metadata(KindMetadata::new(kind.schema_id, kind.data)?);
        }

        builder.build()
    }
}

/// 资源元数据响应。
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "schema_version": 1,
    "summary": {
        "description": "Human readable resource description",
        "tags": ["demo", "asset"]
    },
    "kind": {
        "schema_id": "test:metadata@1",
        "data": {
            "source": "swagger"
        }
    }
}))]
pub(crate) struct ResourceMetadataResponse {
    /// 元数据结构版本，由服务端维护。
    pub(crate) schema_version: u32,
    /// 核心摘要元数据。
    pub(crate) summary: ResourceSummaryMetadataResponse,
    /// kind/plugin 专属元数据。
    pub(crate) kind: Option<KindMetadataResponse>,
}

/// 资源核心摘要元数据响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceSummaryMetadataResponse {
    /// 资源描述。
    pub(crate) description: Option<String>,
    /// 资源标签。
    pub(crate) tags: Vec<String>,
}

/// kind/plugin 专属元数据响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct KindMetadataResponse {
    /// 插件 schema 标识。
    pub(crate) schema_id: String,
    /// kind/plugin 专属数据。
    pub(crate) data: Value,
}

impl From<&ResourceMetadata> for ResourceMetadataResponse {
    fn from(metadata: &ResourceMetadata) -> Self {
        Self {
            schema_version: metadata.schema_version(),
            summary: ResourceSummaryMetadataResponse {
                description: metadata.description().map(str::to_string),
                tags: metadata.tags().to_vec(),
            },
            kind: metadata.kind_metadata().map(|kind| KindMetadataResponse {
                schema_id: kind.schema_id().to_string(),
                data: kind.data().clone(),
            }),
        }
    }
}

/// 资源响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceResponse {
    /// 资源唯一标识。
    pub(crate) id: String,
    /// 资源展示名。
    pub(crate) name: String,
    /// 资源类型。
    pub(crate) kind: String,
    /// 资源生命周期状态。
    pub(crate) status: String,
    /// 资源元数据。
    pub(crate) metadata: ResourceMetadataResponse,
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
    /// 是否允许下载原始内容。
    pub(crate) download_content: bool,
    /// 是否允许在线阅读文本。
    pub(crate) read: bool,
    /// 是否允许以内联方式查看内容。
    pub(crate) view_inline: bool,
    /// 是否允许预览。
    pub(crate) preview: bool,
    /// 是否允许缩略图。
    pub(crate) thumbnail: bool,
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
}

impl From<&ResourceActionOutput> for ResourceActionOutputResponse {
    fn from(output: &ResourceActionOutput) -> Self {
        Self {
            resource_id: output.resource_id().to_string(),
            action: output.action().as_str().to_string(),
            view: serde_json::to_value(&output.output().view)
                .expect("plugin view should serialize to JSON"),
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
            kind: resource.kind().as_str().to_string(),
            status: status_text(resource.status()).to_string(),
            metadata: ResourceMetadataResponse::from(resource.metadata()),
            content: resource.content().map(ResourceContentResponse::from),
            actions: ResourceActionsResponse::from(actions),
            created_at: resource.created_at().to_rfc3339(),
            updated_at: resource.updated_at().to_rfc3339(),
            deleted_at: resource.deleted_at().map(|value| value.to_rfc3339()),
        }
    }
}

impl From<ResourceActions> for ResourceActionsResponse {
    fn from(actions: ResourceActions) -> Self {
        Self {
            download_content: actions.download_content(),
            read: actions.read(),
            view_inline: actions.view_inline(),
            preview: actions.preview(),
            thumbnail: actions.thumbnail(),
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
    /// 内容在存储系统中的定位键。
    pub(crate) key: String,
    /// 内容字节大小。
    pub(crate) size: u64,
    /// 内容 MIME 类型。
    pub(crate) mime_type: Option<String>,
    /// 上传时的原始文件名。
    pub(crate) original_filename: Option<String>,
    /// 内容校验和集合。
    pub(crate) checksum: Vec<ChecksumResponse>,
}

impl From<&ResourceContent> for ResourceContentResponse {
    fn from(content: &ResourceContent) -> Self {
        Self {
            key: content.key().as_str().to_string(),
            size: content.size(),
            mime_type: content.mime_type().map(str::to_string),
            original_filename: content.original_filename().map(str::to_string),
            checksum: content
                .checksums()
                .iter()
                .map(ChecksumResponse::from)
                .collect(),
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
            kind: checksum_kind_text(checksum.kind()).to_string(),
            value: checksum.value().to_string(),
        }
    }
}

fn status_text(status: ResourceStatus) -> &'static str {
    match status {
        ResourceStatus::Active => "active",
        ResourceStatus::Archived => "archived",
    }
}

fn checksum_kind_text(kind: ChecksumKind) -> &'static str {
    match kind {
        ChecksumKind::Sha256 => "sha256",
    }
}
