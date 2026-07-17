use super::{
    ResourceContent, ResourceDirectory, ResourceKind, ResourceMetadata, ResourceMetadataPatch,
    ResourceStatus, normalize_required_text,
};
use crate::error::ResourceError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 资源名称允许的最大字符数。
const MAX_RESOURCE_NAME_LEN: usize = 255;

// ==================================================
// 核心聚合根
// ==================================================

crate::gen_id_uuid_v7!(ResourceId);

/// 资源聚合根。
///
/// `Resource` 负责维护资源基础信息、元数据、内容引用和生命周期状态。
/// 外部代码应通过构建器和行为方法修改资源，避免绕过领域规则直接写字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// 资源唯一标识。
    id: ResourceId,
    /// 资源展示名，用于检索、展示和人工识别。
    name: String,
    /// 资源所在的规范化逻辑目录。
    directory: ResourceDirectory,
    /// 资源类型，用于区分图片、文档、音频等不同业务资源。
    kind: ResourceKind,
    /// 资源生命周期状态，不包含软删除状态。
    status: ResourceStatus,
    /// 资源动态元数据，承载业务扩展字段。
    metadata: ResourceMetadata,
    /// 资源内容引用；纯元数据资源可以没有内容。
    content: Option<ResourceContent>,
    /// 资源创建时间。
    created_at: DateTime<Utc>,
    /// 资源最后更新时间。
    updated_at: DateTime<Utc>,
    /// 软删除时间；为空表示未删除。
    deleted_at: Option<DateTime<Utc>>,
}

/// 资源聚合快照。
///
/// 该结构用于持久化适配器从数据库记录还原 `Resource` 聚合，不作为普通业务创建入口。
/// 普通业务创建应继续使用 `Resource::builder()`，以便由领域模型分配新 ID 和时间戳。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    /// 资源唯一标识。
    pub id: ResourceId,
    /// 资源展示名。
    pub name: String,
    /// 资源所在的规范化逻辑目录。
    pub directory: ResourceDirectory,
    /// 资源类型。
    pub kind: ResourceKind,
    /// 资源生命周期状态。
    pub status: ResourceStatus,
    /// 资源动态元数据。
    pub metadata: ResourceMetadata,
    /// 资源内容引用。
    pub content: Option<ResourceContent>,
    /// 资源创建时间。
    pub created_at: DateTime<Utc>,
    /// 资源最后更新时间。
    pub updated_at: DateTime<Utc>,
    /// 软删除时间。
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Resource {
    /// 创建资源构建器。
    pub fn builder(name: impl Into<String>) -> ResourceBuilder {
        ResourceBuilder::new(name)
    }

    /// 从持久化快照还原资源聚合。
    ///
    /// 该方法会保留快照中的 ID、时间戳和软删除状态，但仍会重新执行资源名称和类型校验。
    /// Repository 实现应通过它还原数据库记录，避免绕过领域约束直接构造 `Resource`。
    pub fn rehydrate(snapshot: ResourceSnapshot) -> Result<Self, ResourceError> {
        let name = normalize_resource_name(snapshot.name)?;
        snapshot.kind.validate()?;
        snapshot.metadata.validate_for_kind(&snapshot.kind)?;

        Ok(Self {
            id: snapshot.id,
            name,
            directory: snapshot.directory,
            kind: snapshot.kind,
            status: snapshot.status,
            metadata: snapshot.metadata,
            content: snapshot.content,
            created_at: snapshot.created_at,
            updated_at: snapshot.updated_at,
            deleted_at: snapshot.deleted_at,
        })
    }

    /// 返回资源唯一标识。
    pub fn id(&self) -> ResourceId {
        self.id
    }

    /// 返回资源展示名。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回资源所在逻辑目录。
    pub fn directory(&self) -> &ResourceDirectory {
        &self.directory
    }

    /// 返回资源类型。
    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    /// 返回资源生命周期状态。
    pub fn status(&self) -> ResourceStatus {
        self.status
    }

    /// 返回资源元数据。
    pub fn metadata(&self) -> &ResourceMetadata {
        &self.metadata
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

    /// 返回资源软删除时间。
    pub fn deleted_at(&self) -> Option<DateTime<Utc>> {
        self.deleted_at
    }

    /// 判断资源是否处于未删除且活跃的状态。
    pub fn is_active(&self) -> bool {
        self.deleted_at.is_none() && matches!(self.status, ResourceStatus::Active)
    }

    /// 判断资源是否处于未删除且已归档的状态。
    pub fn is_archived(&self) -> bool {
        self.deleted_at.is_none() && matches!(self.status, ResourceStatus::Archived)
    }

    /// 是否已被软删除
    ///
    /// 当 `deleted_at` 字段不为空时，表示为已经软删除。
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// 重命名资源。
    ///
    /// 名称会被去除首尾空白，并按照资源名称规则校验。
    pub fn rename(&mut self, name: impl Into<String>) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;
        let name = normalize_resource_name(name.into())?;

        if self.name != name {
            self.name = name;
            self.touch();
        }

        Ok(())
    }

    /// 移动资源到新的逻辑目录。
    pub fn move_to_directory(&mut self, directory: ResourceDirectory) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;

        if self.directory != directory {
            self.directory = directory;
            self.touch();
        }

        Ok(())
    }

    /// 修改资源类型。
    ///
    /// 已删除资源不能修改类型，新类型必须满足 `ResourceKind` 校验规则。
    pub fn change_kind(&mut self, kind: impl Into<ResourceKind>) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;
        let kind = kind.into();
        kind.validate()?;

        if self.kind != kind {
            self.kind = kind;
            self.metadata.clear_kind_metadata();
            self.touch();
        }

        Ok(())
    }

    /// 将资源归档。
    ///
    /// 归档后的资源仍然存在，但不再被视为活跃资源。
    pub fn archive(&mut self) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;

        if !self.status.is_archived() {
            self.status = ResourceStatus::Archived;
            self.touch();
        }

        Ok(())
    }

    /// 将资源恢复为活跃状态。
    pub fn activate(&mut self) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;

        if !self.status.is_active() {
            self.status = ResourceStatus::Active;
            self.touch();
        }

        Ok(())
    }

    /// 软删除资源。
    ///
    /// 软删除会记录 `deleted_at` 并刷新 `updated_at`，不会清除内容引用和元数据。
    pub fn soft_delete(&mut self) {
        if self.deleted_at.is_none() {
            let now = Utc::now();
            self.deleted_at = Some(now);
            self.updated_at = now;
        }
    }

    /// 从软删除状态恢复资源。
    pub fn restore(&mut self) {
        if self.deleted_at.take().is_some() {
            self.touch();
        }
    }

    /// 替换资源元数据。
    pub fn set_metadata(&mut self, metadata: ResourceMetadata) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;
        metadata.validate_for_kind(&self.kind)?;
        if self.metadata != metadata {
            self.metadata = metadata;
            self.touch();
        }
        Ok(())
    }

    /// 部分更新资源元数据，未出现在补丁中的分区保持不变。
    pub fn patch_metadata(&mut self, patch: ResourceMetadataPatch) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;
        if self.metadata.apply_patch(patch, &self.kind)? {
            self.touch();
        }
        Ok(())
    }

    /// 绑定或替换资源内容引用。
    pub fn attach_content(&mut self, content: ResourceContent) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;
        self.content = Some(content);
        self.touch();
        Ok(())
    }

    /// 解绑资源内容引用，并返回原内容引用。
    pub fn detach_content(&mut self) -> Result<Option<ResourceContent>, ResourceError> {
        self.ensure_not_deleted()?;
        let content = self.content.take();

        if content.is_some() {
            self.touch();
        }

        Ok(content)
    }

    /// 确认资源当前仍可被修改。
    fn ensure_not_deleted(&self) -> Result<(), ResourceError> {
        if self.is_deleted() {
            Err(ResourceError::DeletedResource)
        } else {
            Ok(())
        }
    }

    /// 刷新资源更新时间。
    fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

/// 归一化并校验资源名称。
fn normalize_resource_name(value: String) -> Result<String, ResourceError> {
    normalize_required_text("resource.name", &value, MAX_RESOURCE_NAME_LEN)
}

/// 资源构建器。
///
/// 用于统一创建包含可选元数据和可选内容引用的 `Resource`。
#[derive(Debug, Clone)]
pub struct ResourceBuilder {
    /// 资源展示名。
    name: String,
    /// 资源类型。
    kind: ResourceKind,
    /// 初始生命周期状态。
    status: ResourceStatus,
    /// 初始逻辑目录。
    directory: ResourceDirectory,
    /// 初始元数据。
    metadata: ResourceMetadata,
    /// 初始内容引用。
    content: Option<ResourceContent>,
}

impl ResourceBuilder {
    /// 创建资源构建器。
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: ResourceKind::default(),
            status: ResourceStatus::default(),
            directory: ResourceDirectory::root(),
            metadata: ResourceMetadata::default(),
            content: None,
        }
    }

    /// 设置资源类型。
    pub fn with_kind(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind = kind.into();
        self
    }

    /// 设置初始生命周期状态。
    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = status;
        self
    }

    /// 设置初始逻辑目录。
    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = directory;
        self
    }

    /// 设置初始元数据。
    pub fn with_metadata(mut self, metadata: impl Into<ResourceMetadata>) -> Self {
        self.metadata = metadata.into();
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
        self.kind.validate()?;
        self.metadata.validate_for_kind(&self.kind)?;
        let now = Utc::now();

        Ok(Resource {
            id: ResourceId::new(),
            name,
            directory: self.directory,
            kind: self.kind,
            status: self.status,
            metadata: self.metadata,
            content: self.content,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }
}
