use asset_core::ResourceError;
use asset_core::domain::{
    Checksum, ChecksumKind, Resource, ResourceContent, ResourceMetadata, ResourceStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use utoipa::{IntoParams, ToSchema};

/// OpenAPI 中表示原始二进制请求或响应体的 schema。
#[derive(Debug, ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub(crate) struct BinaryContent(Vec<u8>);

/// 创建纯元数据资源请求。
#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateResourceRequest {
    /// 资源展示名。
    pub(crate) name: String,
    /// 可选资源类型。
    pub(crate) kind: Option<String>,
    /// 可选初始状态：`active` 或 `archived`。
    pub(crate) status: Option<String>,
    /// 可选资源元数据；扩展字段应放入 `attributes`。
    pub(crate) metadata: Option<ResourceMetadataRequest>,
}

/// 上传内容并创建资源请求。
#[derive(Debug, Deserialize, ToSchema)]
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
    /// 可选资源元数据；扩展字段应放入 `attributes`。
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

impl HealthResponse {
    /// 构造正常服务状态响应。
    pub(crate) fn ok() -> Self {
        Self {
            status: "ok".to_string(),
        }
    }
}

/// 创建或上传资源时可传入的资源元数据。
///
/// 服务端只接受该结构作为元数据入口。暂时没有被提升为一等字段的业务属性应放入
/// `attributes`，不要直接把任意 JSON 当作整段 metadata 传入。
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub(crate) struct ResourceMetadataRequest {
    /// 资源描述。
    pub(crate) description: Option<String>,
    /// 资源标签。
    pub(crate) tags: Option<Vec<String>>,
    /// 扩展属性。
    pub(crate) attributes: Option<BTreeMap<String, Value>>,
}

impl ResourceMetadataRequest {
    /// 转换为领域元数据并执行领域校验。
    pub(crate) fn into_domain(self) -> Result<ResourceMetadata, ResourceError> {
        let mut builder = ResourceMetadata::builder();

        if let Some(description) = self.description {
            builder = builder.with_description(description);
        }

        if let Some(tags) = self.tags {
            builder = builder.with_tags(tags);
        }

        if let Some(attributes) = self.attributes {
            builder = builder.with_attributes(attributes);
        }

        builder.build()
    }
}

/// 资源元数据响应。
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResourceMetadataResponse {
    /// 元数据结构版本，由服务端维护。
    pub(crate) schema_version: u32,
    /// 资源描述。
    pub(crate) description: Option<String>,
    /// 资源标签。
    pub(crate) tags: Vec<String>,
    /// 扩展属性。
    pub(crate) attributes: BTreeMap<String, Value>,
}

impl From<&ResourceMetadata> for ResourceMetadataResponse {
    fn from(metadata: &ResourceMetadata) -> Self {
        Self {
            schema_version: metadata.schema_version(),
            description: metadata.description().map(str::to_string),
            tags: metadata.tags().to_vec(),
            attributes: metadata.attributes().clone(),
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
    /// 资源创建时间，RFC3339 格式。
    pub(crate) created_at: String,
    /// 资源最后更新时间，RFC3339 格式。
    pub(crate) updated_at: String,
    /// 软删除时间，RFC3339 格式；为空表示未删除。
    pub(crate) deleted_at: Option<String>,
}

impl From<&Resource> for ResourceResponse {
    fn from(resource: &Resource) -> Self {
        Self {
            id: resource.id().to_string(),
            name: resource.name().to_string(),
            kind: resource.kind().as_str().to_string(),
            status: status_text(resource.status()).to_string(),
            metadata: ResourceMetadataResponse::from(resource.metadata()),
            content: resource.content().map(ResourceContentResponse::from),
            created_at: resource.created_at().to_rfc3339(),
            updated_at: resource.updated_at().to_rfc3339(),
            deleted_at: resource.deleted_at().map(|value| value.to_rfc3339()),
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
        ResourceStatus::Active => "Active",
        ResourceStatus::Archived => "Archived",
    }
}

fn checksum_kind_text(kind: ChecksumKind) -> &'static str {
    match kind {
        ChecksumKind::Sha256 => "sha256",
    }
}
