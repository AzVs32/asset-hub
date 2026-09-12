use asset_core::{
    directory::domain::DirectoryPath,
    resource::domain::{
        Checksum, ContentVerificationStatus, Resource, ResourceContent, ResourceEffectiveStatus,
        ResourceLifecycleStatus,
    },
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
    /// Stable global parent Directory ID.
    pub(crate) parent_id: String,
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
    /// 可选名称模糊搜索关键字。
    pub(crate) q: Option<String>,
    /// 相对于全局根目录的过滤路径；根目录为空字符串。
    #[param(value_type = Option<String>)]
    pub(crate) directory: Option<DirectoryPath>,
}

/// 目录浏览查询参数。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListDirectoryQuery {
    /// 相对于全局根目录的路径；根目录为空字符串。
    #[param(value_type = Option<String>)]
    pub(crate) path: Option<DirectoryPath>,
    /// 资源页码，从 1 开始。
    pub(crate) page: Option<u32>,
    /// 每页资源数量。
    pub(crate) limit: Option<u32>,
    /// 可选名称模糊搜索关键字。
    pub(crate) q: Option<String>,
}

/// 更新资源请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(example = json!({"name": "renamed.txt"}))]
pub(crate) struct UpdateResourceRequest {
    /// Required optimistic-concurrency precondition.
    pub(crate) expected_revision: u64,
    /// 可选新资源展示名。
    pub(crate) name: Option<String>,
    /// Optional stable destination Directory UUID.
    pub(crate) directory_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateDirectoryRequest {
    pub(crate) expected_revision: u64,
    pub(crate) name: Option<String>,
    pub(crate) parent_id: Option<String>,
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
    pub(crate) mime_type: Option<String>,
    pub(crate) size: u64,
    /// 客户端对完整本地文件增量计算出的 SHA-256。
    pub(crate) expected_sha256: String,
}

/// 为已有资源创建分块内容替换会话。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateContentReplacementUploadRequest {
    pub(crate) expected_revision: u64,
    pub(crate) mime_type: Option<String>,
    pub(crate) size: u64,
    /// 客户端对完整替换内容增量计算出的 SHA-256。
    pub(crate) expected_sha256: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct UploadSessionResponse {
    pub(crate) id: String,
    pub(crate) offset: u64,
    pub(crate) size: u64,
    /// uploading、finalizing、completed 或 failed。
    pub(crate) status: String,
    /// finalization 完成后创建或更新的 Resource ID。
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
    pub(crate) blob_storage: HealthComponentResponse,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HealthComponentResponse {
    pub(crate) status: String,
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
    /// Stable Directory identity; `directory` is the global relative display path.
    pub(crate) directory_id: String,
    /// 相对于全局根目录的路径；根目录为空字符串。
    #[schema(value_type = String)]
    pub(crate) directory: DirectoryPath,
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
    /// 相对于全局根目录的路径。
    pub(crate) path: String,
    /// 相对于全局根目录的父路径。
    pub(crate) parent_path: String,
    /// 当前目录名。
    pub(crate) name: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) revision: u64,
}

/// 目录浏览响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DirectoryListingResponse {
    /// 相对于全局根目录的当前路径。
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
