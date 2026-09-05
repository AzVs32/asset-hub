use asset_core::domain::{
    Checksum, ContentVerificationStatus, DefinitionOrigin, DirectoryKindDefinition, DirectoryPath,
    Resource, ResourceContent, ResourceEffectiveStatus, ResourceKindDefinition,
    ResourceLifecycleStatus,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// OpenAPI 中表示原始二进制请求或响应体的 schema。
#[derive(Debug, ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub(crate) struct BinaryContent(Vec<u8>);

/// 创建逻辑目录请求。
#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateDirectoryRequest {
    /// Stable parent Directory ID.
    pub(crate) parent_id: String,
    /// 新目录名称，只允许单个路径段。
    pub(crate) name: String,
    /// 可选目录类型。
    pub(crate) kind: Option<String>,
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
    /// 可选名称模糊搜索关键字。
    pub(crate) q: Option<String>,
    /// 相对于当前用户可见根目录的过滤路径；根目录为空字符串。
    #[param(value_type = Option<String>)]
    pub(crate) directory: Option<DirectoryPath>,
}

/// 目录浏览查询参数。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListDirectoryQuery {
    /// 相对于当前用户可见根目录的路径；根目录为空字符串。
    #[param(value_type = Option<String>)]
    pub(crate) path: Option<DirectoryPath>,
    /// 资源页码，从 1 开始。
    pub(crate) page: Option<u32>,
    /// 每页资源数量。
    pub(crate) limit: Option<u32>,
    /// 可选资源类型过滤。
    pub(crate) kind: Option<String>,
    /// 可选名称模糊搜索关键字。
    pub(crate) q: Option<String>,
}

/// 更新资源请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(example = json!({
    "name": "renamed.txt",
    "kind": "core:resource"
}))]
pub(crate) struct UpdateResourceRequest {
    /// Required optimistic-concurrency precondition.
    pub(crate) expected_revision: u64,
    /// 可选新资源展示名。
    pub(crate) name: Option<String>,
    /// 可选新资源类型。
    pub(crate) kind: Option<String>,
    /// Optional stable destination Directory UUID.
    pub(crate) directory_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateDirectoryRequest {
    pub(crate) expected_revision: u64,
    pub(crate) name: Option<String>,
    pub(crate) parent_id: Option<String>,
    pub(crate) kind: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ExpectedRevisionQuery {
    pub(crate) expected_revision: u64,
}

/// 创建断点续传会话。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateUploadRequest {
    pub(crate) name: String,
    #[serde(default)]
    #[schema(value_type = String)]
    pub(crate) directory_id: String,
    pub(crate) kind: Option<String>,
    pub(crate) mime_type: Option<String>,
    pub(crate) size: u64,
    /// 客户端对完整本地文件增量计算出的 SHA-256。
    pub(crate) expected_sha256: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct UploadSessionResponse {
    pub(crate) id: String,
    pub(crate) offset: u64,
    pub(crate) size: u64,
    /// uploading、finalizing、completed 或 failed。
    pub(crate) status: String,
    /// finalization 完成后创建的 Resource ID。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resource_id: Option<String>,
    /// 后台 finalization 的失败原因。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
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
}

/// 健康检查响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HealthResponse {
    /// 服务状态。
    pub(crate) status: String,
    pub(crate) database: HealthComponentResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_store: Option<HealthComponentResponse>,
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

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DirectoryKindsResponse {
    pub(crate) items: Vec<DirectoryKindResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct DirectoryKindResponse {
    pub(crate) kind: String,
    pub(crate) parent: Option<String>,
    pub(crate) ancestors: Vec<String>,
    pub(crate) allowed_parent_kinds: Vec<String>,
    pub(crate) label: String,
    pub(crate) origin: DefinitionOriginResponse,
}

impl DirectoryKindResponse {
    pub(crate) fn from_definition(
        definition: &DirectoryKindDefinition,
        service: &asset_core::service::DirectoryService,
    ) -> Self {
        Self {
            kind: definition.kind().as_str().to_string(),
            parent: definition.parent().map(|kind| kind.as_str().to_string()),
            ancestors: service
                .kind_lineage(definition.kind())
                .into_iter()
                .skip(1)
                .map(|kind| kind.as_str().to_string())
                .collect(),
            allowed_parent_kinds: definition
                .allowed_parent_kinds()
                .iter()
                .map(|kind| kind.as_str().to_string())
                .collect(),
            label: definition.label().to_string(),
            origin: DefinitionOriginResponse::from(definition.origin()),
        }
    }
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
    /// 当前仅使用内置定义来源 `builtin`。
    pub(crate) origin: DefinitionOriginResponse,
}

impl ResourceKindResponse {
    pub(crate) fn from_definition(
        definition: &ResourceKindDefinition,
        service: &asset_core::service::ResourceService,
    ) -> Self {
        Self {
            kind: definition.kind().as_str().to_string(),
            parent: definition.parent().map(|parent| parent.as_str().to_owned()),
            ancestors: service
                .kind_lineage(definition.kind())
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
            origin: DefinitionOriginResponse::from(definition.origin()),
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct DefinitionOriginResponse {
    pub(crate) kind: String,
    pub(crate) id: String,
}

impl From<&DefinitionOrigin> for DefinitionOriginResponse {
    fn from(origin: &DefinitionOrigin) -> Self {
        Self {
            kind: origin.kind().to_string(),
            id: origin.id().to_string(),
        }
    }
}

/// 内容匹配条件。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResourceContentMatcherResponse {
    /// 匹配的 MIME 类型，支持 `image/*` 这类通配前缀。
    pub(crate) mime_types: Vec<String>,
    /// 匹配的文件扩展名。
    pub(crate) extensions: Vec<String>,
}

impl HealthResponse {
    pub(crate) fn new(
        database_ready: bool,
        blob_storage_ready: bool,
        session_store_ready: Option<bool>,
    ) -> Self {
        let session_ready = session_store_ready.unwrap_or(true);
        Self {
            status: if database_ready && blob_storage_ready && session_ready {
                "ready"
            } else {
                "unavailable"
            }
            .to_string(),
            database: HealthComponentResponse {
                status: component_status(database_ready),
            },
            session_store: session_store_ready.map(|ready| HealthComponentResponse {
                status: component_status(ready),
            }),
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
    /// Stable Directory identity; `directory` remains only the caller-relative display path.
    pub(crate) directory_id: String,
    /// 相对于当前用户可见根目录的路径；根目录为空字符串。
    #[schema(value_type = String)]
    pub(crate) directory: DirectoryPath,
    /// 资源类型。
    pub(crate) kind: String,
    /// 由 Core 统一派生的资源生命周期、内容和有效状态。
    pub(crate) state: ResourceStateResponse,
    /// 资源内容引用。
    pub(crate) content: Option<ResourceContentResponse>,
    /// 资源创建时间，RFC3339 格式。
    pub(crate) created_at: String,
    /// 资源最后更新时间，RFC3339 格式。
    pub(crate) updated_at: String,
    /// 单调递增的资源聚合版本。
    pub(crate) revision: u64,
}

/// 资源生命周期、内容和单值有效状态的统一响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceStateResponse {
    pub(crate) lifecycle: ResourceLifecycleStateResponse,
    pub(crate) content: ResourceContentStateResponse,
    pub(crate) effective: ResourceEffectiveStateResponse,
}

/// 资源生命周期。
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum ResourceLifecycleStateResponse {
    Active,
}

/// 资源是否包含对象内容及其校验状态。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResourceContentStateResponse {
    Absent,
    Pending,
    Verified,
    Failed,
}

/// 需要单值状态判断的消费者所使用的统一有效状态。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResourceEffectiveStateResponse {
    NoContent,
    Verifying,
    Ready,
    VerificationFailed,
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
pub(crate) struct DirectoryResponse {
    /// 稳定目录标识；目录移动或重命名后保持不变。
    pub(crate) id: String,
    pub(crate) parent_id: Option<String>,
    /// 相对于当前用户可见根目录的路径。
    pub(crate) path: String,
    /// 相对于当前用户可见根目录的父路径。
    pub(crate) parent_path: String,
    /// 当前目录名。
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) revision: u64,
}

/// 目录浏览响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DirectoryListingResponse {
    /// 相对于当前用户可见根目录的当前路径。
    #[schema(value_type = String)]
    pub(crate) path: DirectoryPath,
    /// 当前目录。
    pub(crate) directory: DirectoryResponse,
    /// 直接子目录。
    pub(crate) folders: Vec<DirectoryResponse>,
    /// 当前目录下的资源分页。
    pub(crate) resources: ResourcePageResponse,
}

impl ResourceResponse {
    pub(crate) fn new(resource: &Resource, directory: DirectoryPath) -> Self {
        Self {
            id: resource.id().to_string(),
            name: resource.name().to_string(),
            directory_id: resource.directory_id().to_string(),
            directory,
            kind: resource.kind().as_str().to_string(),
            state: ResourceStateResponse::from(resource),
            content: resource.content().map(ResourceContentResponse::from),
            created_at: resource.created_at().to_rfc3339(),
            updated_at: resource.updated_at().to_rfc3339(),
            revision: resource.revision(),
        }
    }
}

impl From<&Resource> for ResourceStateResponse {
    fn from(resource: &Resource) -> Self {
        let state = resource.state();
        let lifecycle = match state.lifecycle() {
            ResourceLifecycleStatus::Active => ResourceLifecycleStateResponse::Active,
        };
        let content = match state.content() {
            None => ResourceContentStateResponse::Absent,
            Some(ContentVerificationStatus::Pending) => ResourceContentStateResponse::Pending,
            Some(ContentVerificationStatus::Verified) => ResourceContentStateResponse::Verified,
            Some(ContentVerificationStatus::Failed) => ResourceContentStateResponse::Failed,
        };
        let effective = match state.effective() {
            ResourceEffectiveStatus::NoContent => ResourceEffectiveStateResponse::NoContent,
            ResourceEffectiveStatus::Verifying => ResourceEffectiveStateResponse::Verifying,
            ResourceEffectiveStatus::Ready => ResourceEffectiveStateResponse::Ready,
            ResourceEffectiveStatus::VerificationFailed => {
                ResourceEffectiveStateResponse::VerificationFailed
            }
        };
        Self {
            lifecycle,
            content,
            effective,
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
    /// 服务端根据内容本体计算得到的校验和；待校验或校验失败时为空。
    pub(crate) checksum: Option<ChecksumResponse>,
    /// 后台校验失败原因；仅校验失败时存在。
    pub(crate) verification_error: Option<String>,
}

impl From<&ResourceContent> for ResourceContentResponse {
    fn from(content: &ResourceContent) -> Self {
        Self {
            size: content.size(),
            mime_type: content.mime_type().map(str::to_string),
            checksum: content.checksum().map(ChecksumResponse::from),
            verification_error: content.verification_error().map(str::to_string),
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
