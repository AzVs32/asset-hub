//! 资源聚合及其内容、类型和状态值对象。
//!
//! 资源生命周期与内容校验是两个独立的状态轴；[`ResourceState`] 在读取时统一投影它们。
//! 该投影不持久化，也不接收状态写命令。尚未发布为 Resource 的上传流程继续由独立的
//! [`crate::domain::UploadSession`] 聚合管理。

mod content;
mod kind;
mod state;

use crate::domain::DirectoryId;
use crate::error::ResourceError;
use chrono::{DateTime, Utc};
use serde::Serialize;

pub use content::{
    Checksum, ChecksumKind, ContentVerificationStatus, ResourceContent, ResourceContentBuilder,
    StorageKey,
};
pub use kind::ResourceKind;
pub use state::{ResourceEffectiveStatus, ResourceLifecycleStatus, ResourceState};

/// 资源名称允许的最大字符数。
const MAX_RESOURCE_NAME_LEN: usize = 255;

// ==================================================
// 核心聚合根
// ==================================================

crate::gen_id_uuid_v7!(ResourceId);

/// 资源聚合根。
///
/// `Resource` 负责维护资源基础信息和内容引用。
/// 外部代码应通过构建器和行为方法修改资源，避免绕过领域规则直接写字段。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resource {
    /// 资源唯一标识。
    id: ResourceId,
    /// 资源文件名；与目录共同构成资源及其 Blob 的唯一规范路径。
    name: String,
    /// 资源所在目录的稳定标识。
    directory_id: DirectoryId,
    /// 资源类型，用于区分图片、文档、音频等不同业务资源。
    kind: ResourceKind,
    /// 资源内容引用；资源可以不包含对象内容。
    content: Option<ResourceContent>,
    /// 资源创建时间。
    created_at: DateTime<Utc>,
    /// 资源最后更新时间。
    updated_at: DateTime<Utc>,
    /// 单调递增的聚合版本，用于乐观并发控制。
    revision: u64,
}

impl Resource {
    /// 创建资源构建器。
    pub fn builder(name: impl Into<String>) -> ResourceBuilder {
        ResourceBuilder::new(name)
    }

    /// 从持久化适配器已解析的完整状态还原资源聚合。
    ///
    /// 该方法保留原 ID 和时间戳，但仍会重新执行聚合约束校验。
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: ResourceId,
        name: String,
        directory_id: DirectoryId,
        kind: ResourceKind,
        content: Option<ResourceContent>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        revision: u64,
    ) -> Result<Self, ResourceError> {
        let name = normalize_resource_name(name)?;
        if revision == 0 {
            return Err(ResourceError::InvalidFormat {
                field: "resource.revision",
                reason: "resource revision must be greater than zero",
            });
        }
        if updated_at < created_at {
            return Err(ResourceError::InvalidFormat {
                field: "resource.updated_at",
                reason: "updated timestamp cannot precede creation",
            });
        }

        Ok(Self {
            id,
            name,
            directory_id,
            kind,
            content,
            created_at,
            updated_at,
            revision,
        })
    }
}

impl Resource {
    /// 返回资源唯一标识。
    pub fn id(&self) -> ResourceId {
        self.id
    }

    /// 返回资源文件名。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回资源所在目录的稳定标识。
    pub fn directory_id(&self) -> DirectoryId {
        self.directory_id
    }

    /// 返回资源类型。
    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    /// 返回资源内容引用。
    pub fn content(&self) -> Option<&ResourceContent> {
        self.content.as_ref()
    }

    /// 返回资源创建时间。
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// 返回资源最后更新时间。
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// 返回单调递增的聚合版本。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 返回生命周期与内容校验状态的统一只读投影。
    ///
    /// 投影不引入新的持久化状态；需要改变资源时仍应调用对应的领域行为和应用服务用例。
    pub fn state(&self) -> ResourceState {
        ResourceState::from_resource(self)
    }

    /// 重命名资源。
    ///
    /// 名称会按资源名称规则校验并原样保留，包括其中的首尾空白。
    pub fn rename(&mut self, name: impl Into<String>) -> Result<(), ResourceError> {
        let name = normalize_resource_name(name.into())?;
        if self.name != name {
            self.name = name;
            self.touch();
        }

        Ok(())
    }

    /// 移动资源到新的逻辑目录。
    pub fn move_to_directory(&mut self, directory_id: DirectoryId) -> Result<(), ResourceError> {
        if self.directory_id != directory_id {
            self.directory_id = directory_id;
            self.touch();
        }

        Ok(())
    }

    /// 修改资源类型。
    ///
    pub fn change_kind(&mut self, kind: ResourceKind) -> Result<(), ResourceError> {
        if self.kind != kind {
            self.kind = kind;
            self.touch();
        }

        Ok(())
    }

    /// 绑定或替换资源内容引用。
    pub fn attach_content(&mut self, content: ResourceContent) -> Result<(), ResourceError> {
        self.content = Some(content);
        self.touch();
        Ok(())
    }

    /// 刷新资源更新时间。
    fn touch(&mut self) {
        self.updated_at = Utc::now();
        self.increment_revision();
    }

    fn increment_revision(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("resource revision should not exhaust u64");
    }
}

/// 校验资源名称并原样保留。
fn normalize_resource_name(value: String) -> Result<String, ResourceError> {
    let name = validate_required_text_exact("resource.name", &value, MAX_RESOURCE_NAME_LEN)?;
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(ResourceError::InvalidFormat {
            field: "resource.name",
            reason: "resource name must be a single file name",
        });
    }
    Ok(name)
}

/// 资源构建器。
///
/// 用于统一创建包含可选内容引用的 `Resource`。
#[derive(Debug, Clone)]
pub struct ResourceBuilder {
    /// 由持久化工作流预先分配的资源 ID。
    id: Option<ResourceId>,
    /// 资源展示名。
    name: String,
    /// 资源类型。
    kind: ResourceKind,
    /// 初始逻辑目录。
    directory_id: DirectoryId,
    /// 初始内容引用。
    content: Option<ResourceContent>,
}

impl ResourceBuilder {
    /// 创建资源构建器。
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            kind: ResourceKind::default(),
            directory_id: DirectoryId::root(),
            content: None,
        }
    }

    /// 使用持久化工作流预先分配的资源 ID。
    pub(crate) fn with_id(mut self, id: ResourceId) -> Self {
        self.id = Some(id);
        self
    }

    /// 设置资源类型。
    pub fn with_kind(mut self, kind: ResourceKind) -> Self {
        self.kind = kind;
        self
    }

    /// 设置初始逻辑目录。
    pub fn with_directory_id(mut self, directory_id: DirectoryId) -> Self {
        self.directory_id = directory_id;
        self
    }

    /// 设置初始内容引用。
    pub fn with_content(mut self, content: ResourceContent) -> Self {
        self.content = Some(content);
        self
    }

    /// 完成构建并执行领域校验。
    pub fn build(self) -> Result<Resource, ResourceError> {
        let name = normalize_resource_name(self.name)?;
        let now = Utc::now();

        Ok(Resource {
            id: self.id.unwrap_or_default(),
            name,
            directory_id: self.directory_id,
            kind: self.kind,
            content: self.content,
            created_at: now,
            updated_at: now,
            revision: 1,
        })
    }
}

/// 归一化并校验 Resource 领域模型中的必填文本。
fn normalize_required_text(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, ResourceError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ResourceError::Blank { field });
    }
    if value.chars().count() > max {
        return Err(ResourceError::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(ResourceError::InvalidFormat {
            field,
            reason: "control characters are not allowed",
        });
    }
    Ok(value.to_owned())
}

/// 校验需要原样保存的 Resource 必填文本，不执行首尾裁剪。
fn validate_required_text_exact(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, ResourceError> {
    if value.trim().is_empty() {
        return Err(ResourceError::Blank { field });
    }
    if value.chars().count() > max {
        return Err(ResourceError::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(ResourceError::InvalidFormat {
            field,
            reason: "control characters are not allowed",
        });
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests;
