//! 目录类型注册表端口。

use crate::domain::DirectoryKind;
use std::collections::HashSet;

/// 目录类型定义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryKindDefinition {
    kind: DirectoryKind,
    parent: Option<DirectoryKind>,
    label: String,
    source: String,
}

impl DirectoryKindDefinition {
    pub fn with_source(
        kind: DirectoryKind,
        label: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            parent: None,
            label: label.into(),
            source: source.into(),
        }
    }

    pub fn with_parent(mut self, parent: Option<DirectoryKind>) -> Self {
        self.parent = parent;
        self
    }

    pub fn kind(&self) -> &DirectoryKind {
        &self.kind
    }

    pub fn parent(&self) -> Option<&DirectoryKind> {
        self.parent.as_ref()
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

/// 当前运行时支持的目录类型注册表端口。
///
/// 基础设施适配器负责在启动时聚合、校验并冻结内置及插件贡献的类型定义。
pub trait DirectoryKindRegistry: Send + Sync {
    /// 返回全部已注册定义；切片在注册表生命周期内必须保持有效和稳定。
    fn definitions(&self) -> &[DirectoryKindDefinition];

    /// 按类型标识查找定义；未注册时返回 `None`。
    fn get(&self, kind: &DirectoryKind) -> Option<&DirectoryKindDefinition> {
        self.definitions()
            .iter()
            .find(|definition| definition.kind() == kind)
    }

    /// 判断类型是否已经注册。
    fn supports(&self, kind: &DirectoryKind) -> bool {
        self.get(kind).is_some()
    }

    /// 从具体类型开始返回自身及其全部祖先，遇到未知类型或继承环时停止。
    fn lineage(&self, kind: &DirectoryKind) -> Vec<DirectoryKind> {
        let mut lineage = Vec::new();
        let mut current = Some(kind.clone());
        let mut visited = HashSet::new();
        while let Some(kind) = current {
            if !visited.insert(kind.clone()) {
                break;
            }
            let Some(definition) = self.get(&kind) else {
                break;
            };
            lineage.push(kind);
            current = definition.parent().cloned();
        }
        lineage
    }

    /// 判断 `kind` 是否等于或继承自 `ancestor`。
    fn is_a(&self, kind: &DirectoryKind, ancestor: &DirectoryKind) -> bool {
        self.lineage(kind).iter().any(|item| item == ancestor)
    }

    /// 返回指定类型自身及其全部已注册后代。
    fn descendants(&self, kind: &DirectoryKind) -> Vec<DirectoryKind> {
        self.definitions()
            .iter()
            .filter(|definition| self.is_a(definition.kind(), kind))
            .map(|definition| definition.kind().clone())
            .collect()
    }
}
