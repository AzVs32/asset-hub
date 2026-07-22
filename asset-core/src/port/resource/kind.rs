//! 资源类型注册表端口。
//!
//! 该端口描述核心/应用入口如何发现当前运行时支持的资源类型。默认实现可以是内置静态表，
//! 后续插件系统可以通过同一端口注册和暴露更多 kind。

use crate::domain::ResourceKind;
use asset_plugin_api::ResourceContentMatcher;
use std::collections::HashSet;

/// 资源类型定义。
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceKindDefinition {
    /// 资源类型值，例如 `core:file`、`mindustry:mod`。
    kind: ResourceKind,
    /// 可选父资源类型。父级能力会被后代继承。
    parent: Option<ResourceKind>,
    /// 展示名称。
    label: String,
    /// 是否支持对象内容。
    supports_content: bool,
    /// 文件自动识别规则。
    detect: ResourceContentMatcher,
    /// 定义来源，例如 `builtin`、`config` 或 `plugin:<id>`。
    source: String,
}

impl ResourceKindDefinition {
    /// 创建资源类型定义。
    pub fn new(kind: ResourceKind, label: impl Into<String>, supports_content: bool) -> Self {
        Self::with_source(kind, label, supports_content, "builtin")
    }

    /// 创建带来源的资源类型定义。
    pub fn with_source(
        kind: ResourceKind,
        label: impl Into<String>,
        supports_content: bool,
        source: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            parent: None,
            label: label.into(),
            supports_content,
            detect: ResourceContentMatcher::default(),
            source: source.into(),
        }
    }

    pub fn with_parent(mut self, parent: Option<ResourceKind>) -> Self {
        self.parent = parent;
        self
    }

    /// 设置文件自动识别规则。
    pub fn with_detect(mut self, detect: ResourceContentMatcher) -> Self {
        self.detect = detect;
        self
    }

    /// 返回资源类型。
    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    pub fn parent(&self) -> Option<&ResourceKind> {
        self.parent.as_ref()
    }

    /// 返回展示名称。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 返回是否支持对象内容。
    pub fn supports_content(&self) -> bool {
        self.supports_content
    }

    /// 返回文件自动识别规则。
    pub fn detect(&self) -> &ResourceContentMatcher {
        &self.detect
    }

    /// 返回定义来源。
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// 资源类型注册表。
pub trait ResourceKindRegistry: Send + Sync {
    /// 返回当前运行时支持的资源类型，不复制整个注册表。
    fn definitions(&self) -> &[ResourceKindDefinition];

    /// 按 kind 查找资源类型定义。
    fn get(&self, kind: &ResourceKind) -> Option<&ResourceKindDefinition> {
        self.definitions()
            .iter()
            .find(|definition| definition.kind().as_str() == kind.as_str())
    }

    /// 判断资源类型是否受支持。
    fn supports(&self, kind: &ResourceKind) -> bool {
        self.get(kind).is_some()
    }

    /// 从具体 kind 开始返回完整谱系：自身、父级，直至根节点。
    fn lineage(&self, kind: &ResourceKind) -> Vec<ResourceKind> {
        let mut lineage = Vec::new();
        let mut current = Some(kind.clone());
        let mut visited = HashSet::new();
        while let Some(kind) = current {
            if !visited.insert(kind.as_str().to_owned()) {
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

    fn is_a(&self, kind: &ResourceKind, ancestor: &ResourceKind) -> bool {
        self.lineage(kind).iter().any(|item| item == ancestor)
    }

    fn descendants(&self, kind: &ResourceKind) -> Vec<ResourceKind> {
        self.definitions()
            .iter()
            .filter(|definition| self.is_a(definition.kind(), kind))
            .map(|definition| definition.kind().clone())
            .collect()
    }

    /// 根据内容特征推断父资源类型。
    ///
    /// 检测只参考 kind 自身的 `detect`。格式插件应贡献具体子 kind，而不是借 action
    /// 匹配规则隐式改变资源类型。
    fn detect_content_kind(
        &self,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> Option<ResourceKind> {
        let mut best: Option<(ResourceKind, u8, usize)> = None;

        for definition in self
            .definitions()
            .iter()
            .filter(|definition| definition.supports_content())
        {
            let score = content_match_score(definition.detect(), mime_type, storage_key, 100);

            if score == 0 {
                continue;
            }

            let depth = self.lineage(definition.kind()).len();
            if best.as_ref().is_none_or(|(_, best_score, best_depth)| {
                score > *best_score || (score == *best_score && depth > *best_depth)
            }) {
                best = Some((definition.kind().clone(), score, depth));
            }
        }

        best.map(|(kind, _, _)| kind)
    }
}

fn content_match_score(
    when: &ResourceContentMatcher,
    mime_type: Option<&str>,
    storage_key: Option<&str>,
    base: u8,
) -> u8 {
    if when.mime_types().is_empty() && when.extensions().is_empty() {
        return 0;
    }

    let mut score = 0;
    let mime_type = mime_type.map(|value| value.trim().to_ascii_lowercase());
    if let Some(mime_type) = mime_type.as_deref() {
        for expected in when.mime_types() {
            if expected.ends_with("/*") && mime_matches(expected, mime_type) {
                score = score.max(base + 10);
            } else if mime_matches(expected, mime_type) {
                score = score.max(base + 20);
            }
        }
    }

    let storage_key = storage_key.map(|value| value.to_ascii_lowercase());
    if let Some(storage_key) = storage_key.as_deref() {
        for extension in when.extensions() {
            if storage_key.ends_with(extension) {
                score = score.max(base + 30);
            }
        }
    }

    score
}

fn mime_matches(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }
    expected
        .strip_suffix("/*")
        .is_some_and(|prefix| actual.starts_with(&format!("{prefix}/")))
}
