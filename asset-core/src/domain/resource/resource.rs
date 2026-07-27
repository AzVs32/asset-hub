use super::{
    ResourceContent, ResourceKind, ResourceStatus, ResourceTag, StorageKey,
    validate_required_text_exact,
};
use crate::domain::DirectoryRef;
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
/// `Resource` 负责维护资源基础信息、标签、内容引用和生命周期状态。
/// 外部代码应通过构建器和行为方法修改资源，避免绕过领域规则直接写字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// 资源唯一标识。
    id: ResourceId,
    /// 资源文件名；与目录共同构成资源及其 Blob 的唯一规范路径。
    name: String,
    /// 资源所在目录的稳定引用；路径只是读取时附带的当前位置投影。
    directory: DirectoryRef,
    /// 资源类型，用于区分图片、文档、音频等不同业务资源。
    kind: ResourceKind,
    /// 资源生命周期状态，不包含软删除状态。
    status: ResourceStatus,
    /// 去重并按稳定字典序排列的资源标签集合。
    tags: Vec<ResourceTag>,
    /// 资源内容引用；资源可以不包含对象内容。
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
    /// 资源所在目录的稳定引用。
    pub directory: DirectoryRef,
    /// 资源类型。
    pub kind: ResourceKind,
    /// 资源生命周期状态。
    pub status: ResourceStatus,
    /// 资源标签；从持久化边界进入时会重新归一化和校验。
    pub tags: Vec<String>,
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
    /// 该方法会保留快照中的 ID、时间戳和软删除状态，但仍会重新执行聚合约束校验。
    /// Repository 实现应通过它还原数据库记录，避免绕过领域约束直接构造 `Resource`。
    pub fn rehydrate(snapshot: ResourceSnapshot) -> Result<Self, ResourceError> {
        let name = normalize_resource_name(snapshot.name)?;
        StorageKey::from_resource_path(snapshot.directory.path(), &name)?;
        let tags = normalize_tags(snapshot.tags)?;

        Ok(Self {
            id: snapshot.id,
            name,
            directory: snapshot.directory,
            kind: snapshot.kind,
            status: snapshot.status,
            tags,
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

    /// 返回资源文件名。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回资源所在目录。
    pub fn directory(&self) -> &DirectoryRef {
        &self.directory
    }

    /// 返回由逻辑目录和资源名称唯一派生的对象存储键。
    pub fn storage_key(&self) -> StorageKey {
        StorageKey::from_resource_path(self.directory.path(), &self.name)
            .expect("resource path is validated when the aggregate changes")
    }

    /// 返回资源类型。
    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    /// 返回资源生命周期状态。
    pub fn status(&self) -> ResourceStatus {
        self.status
    }

    /// 返回资源标签。
    pub fn tags(&self) -> &[ResourceTag] {
        &self.tags
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
    /// 名称会按资源名称规则校验并原样保留，包括其中的首尾空白。
    pub fn rename(&mut self, name: impl Into<String>) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;
        let name = normalize_resource_name(name.into())?;
        StorageKey::from_resource_path(self.directory.path(), &name)?;

        if self.name != name {
            self.name = name;
            self.touch();
        }

        Ok(())
    }

    /// 移动资源到新的逻辑目录。
    pub fn move_to_directory(&mut self, directory: DirectoryRef) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;
        StorageKey::from_resource_path(directory.path(), &self.name)?;

        if self.directory != directory {
            self.directory = directory;
            self.touch();
        }

        Ok(())
    }

    /// 修改资源类型。
    ///
    /// 已删除资源不能修改类型。
    pub fn change_kind(&mut self, kind: ResourceKind) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;

        if self.kind != kind {
            self.kind = kind;
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
    /// 软删除会记录 `deleted_at` 并刷新 `updated_at`，不会清除内容引用或标签。
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

    /// 替换全部资源标签；标签会被归一化、去重并按稳定字典序排列。
    pub fn replace_tags(&mut self, tags: Vec<String>) -> Result<(), ResourceError> {
        self.ensure_not_deleted()?;
        let tags = normalize_tags(tags)?;
        if self.tags != tags {
            self.tags = tags;
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

fn normalize_tags(tags: Vec<String>) -> Result<Vec<ResourceTag>, ResourceError> {
    let mut normalized = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = ResourceTag::try_new(tag)?;
        if !normalized.contains(&tag) {
            normalized.push(tag);
        }
    }
    normalized.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));

    Ok(normalized)
}

/// 资源构建器。
///
/// 用于统一创建包含可选标签和内容引用的 `Resource`。
#[derive(Debug, Clone)]
pub struct ResourceBuilder {
    /// 资源展示名。
    name: String,
    /// 资源类型。
    kind: ResourceKind,
    /// 初始生命周期状态。
    status: ResourceStatus,
    /// 初始逻辑目录。
    directory: DirectoryRef,
    /// 初始资源标签。
    tags: Vec<String>,
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
            directory: DirectoryRef::root(),
            tags: Vec::new(),
            content: None,
        }
    }

    /// 设置资源类型。
    pub fn with_kind(mut self, kind: ResourceKind) -> Self {
        self.kind = kind;
        self
    }

    /// 设置初始生命周期状态。
    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = status;
        self
    }

    /// 设置初始逻辑目录。
    pub fn with_directory(mut self, directory: DirectoryRef) -> Self {
        self.directory = directory;
        self
    }

    /// 设置初始资源标签。
    pub fn with_tags<T, I>(mut self, tags: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = T>,
    {
        self.tags = tags.into_iter().map(Into::into).collect();
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
        StorageKey::from_resource_path(self.directory.path(), &name)?;
        let tags = normalize_tags(self.tags)?;
        let now = Utc::now();

        Ok(Resource {
            id: ResourceId::new(),
            name,
            directory: self.directory,
            kind: self.kind,
            status: self.status,
            tags,
            content: self.content,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }
}
