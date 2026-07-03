//! 资源类型注册表端口。
//!
//! 该端口描述核心/应用入口如何发现当前运行时支持的资源类型。默认实现可以是内置静态表，
//! 后续插件系统可以通过同一端口注册和暴露更多 kind。

use crate::domain::ResourceKind;
pub use asset_plugin_api::{
    ResourceAction, ResourceActionAccess, ResourceActionContentDelivery, ResourceActionDefinition,
    ResourceActionWhen,
};
use serde_json::Value;

/// 资源类型定义。
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceKindDefinition {
    /// 资源类型值，例如 `core:file`、`mindustry:mod`。
    kind: ResourceKind,
    /// 展示名称。
    label: String,
    /// 默认 kind metadata schema id。
    schema_id: Option<String>,
    /// kind metadata JSON schema。
    metadata_schema: Option<Value>,
    /// 是否支持对象内容。
    supports_content: bool,
    /// 文件自动识别规则。
    detect: ResourceActionWhen,
    /// kind 支持的动作，例如 `read`、`thumbnail`、`plugin:sync`。
    actions: Vec<ResourceActionDefinition>,
    /// 定义来源，例如 `builtin`、`config` 或 `plugin:<id>`。
    source: String,
}

impl ResourceKindDefinition {
    /// 创建资源类型定义。
    pub fn new(
        kind: ResourceKind,
        label: impl Into<String>,
        schema_id: Option<String>,
        supports_content: bool,
    ) -> Self {
        Self::with_source(kind, label, schema_id, supports_content, "builtin")
    }

    /// 创建带来源的资源类型定义。
    pub fn with_source(
        kind: ResourceKind,
        label: impl Into<String>,
        schema_id: Option<String>,
        supports_content: bool,
        source: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            label: label.into(),
            schema_id,
            metadata_schema: None,
            supports_content,
            detect: ResourceActionWhen::default(),
            actions: Vec::new(),
            source: source.into(),
        }
    }

    /// 设置 kind metadata JSON schema。
    pub fn with_metadata_schema(mut self, metadata_schema: Option<Value>) -> Self {
        self.metadata_schema = metadata_schema;
        self
    }

    /// 设置文件自动识别规则。
    pub fn with_detect(mut self, detect: ResourceActionWhen) -> Self {
        self.detect = detect;
        self
    }

    /// 设置 kind 支持的动作。
    pub fn with_actions(mut self, actions: Vec<ResourceActionDefinition>) -> Self {
        self.actions = actions;
        self
    }

    /// 返回资源类型。
    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    /// 返回展示名称。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 返回默认 schema id。
    pub fn schema_id(&self) -> Option<&str> {
        self.schema_id.as_deref()
    }

    /// 返回 kind metadata JSON schema。
    pub fn metadata_schema(&self) -> Option<&Value> {
        self.metadata_schema.as_ref()
    }

    /// 返回是否支持对象内容。
    pub fn supports_content(&self) -> bool {
        self.supports_content
    }

    /// 返回文件自动识别规则。
    pub fn detect(&self) -> &ResourceActionWhen {
        &self.detect
    }

    /// 返回 kind 支持的动作。
    pub fn actions(&self) -> &[ResourceActionDefinition] {
        &self.actions
    }

    /// 判断 kind 是否支持指定动作。
    pub fn has_action(&self, action: impl AsRef<str>) -> bool {
        let action = action.as_ref();
        self.actions
            .iter()
            .any(|definition| definition.id().as_str() == action)
    }

    /// 返回定义来源。
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// 资源类型注册表。
pub trait ResourceKindRegistry: Send + Sync {
    /// 列出当前运行时支持的资源类型。
    fn list(&self) -> Vec<ResourceKindDefinition>;

    /// 按 kind 查找资源类型定义。
    fn get(&self, kind: &ResourceKind) -> Option<ResourceKindDefinition> {
        self.list()
            .into_iter()
            .find(|definition| definition.kind().as_str() == kind.as_str())
    }

    /// 判断资源类型是否受支持。
    fn supports(&self, kind: &ResourceKind) -> bool {
        self.get(kind).is_some()
    }

    /// 根据内容特征推断父资源类型。
    ///
    /// 该方法同时参考 kind 自身的 `detect` 和挂载在该 kind 上的 action 匹配规则。
    /// 插件贡献的 action 只会让识别结果落到 action 所属的父 kind，而不会产生新的插件 kind。
    fn detect_content_kind(
        &self,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> Option<ResourceKind> {
        let mut best: Option<(ResourceKind, u8)> = None;

        for definition in self
            .list()
            .into_iter()
            .filter(ResourceKindDefinition::supports_content)
        {
            let mut score = content_match_score(definition.detect(), mime_type, storage_key, 100);
            for action in definition.actions() {
                score = score.max(content_match_score(
                    action.when(),
                    mime_type,
                    storage_key,
                    0,
                ));
            }

            if score == 0 {
                continue;
            }

            if best
                .as_ref()
                .is_none_or(|(_, best_score)| score > *best_score)
            {
                best = Some((definition.kind().clone(), score));
            }
        }

        best.map(|(kind, _)| kind)
    }
}

fn content_match_score(
    when: &ResourceActionWhen,
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

    let storage_key = storage_key.map(|value| value.trim().to_ascii_lowercase());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestRegistry {
        definitions: Vec<ResourceKindDefinition>,
    }

    impl ResourceKindRegistry for TestRegistry {
        fn list(&self) -> Vec<ResourceKindDefinition> {
            self.definitions.clone()
        }
    }

    #[test]
    fn detects_parent_kind_from_action_rules() {
        let registry = TestRegistry {
            definitions: vec![
                ResourceKindDefinition::new(ResourceKind::from("core:file"), "File", None, true),
                ResourceKindDefinition::new(
                    ResourceKind::from("core:document"),
                    "Document",
                    None,
                    true,
                )
                .with_actions(vec![
                    ResourceActionDefinition::new("azvs.markdown.render", "Read Markdown")
                        .with_when(
                            ResourceActionWhen::new()
                                .with_kinds(["core:document"])
                                .with_mime_types(["text/markdown"])
                                .with_extensions([".md"]),
                        ),
                ]),
            ],
        };

        assert_eq!(
            registry.detect_content_kind(Some("text/plain"), Some("notes/readme.md")),
            Some(ResourceKind::from("core:document"))
        );
    }

    #[test]
    fn kind_detect_rules_beat_action_rules() {
        let registry = TestRegistry {
            definitions: vec![
                ResourceKindDefinition::new(ResourceKind::from("core:image"), "Image", None, true)
                    .with_detect(ResourceActionWhen::new().with_extensions([".png"])),
                ResourceKindDefinition::new(ResourceKind::from("core:file"), "File", None, true)
                    .with_actions(vec![
                        ResourceActionDefinition::new("demo:png_action", "PNG action")
                            .with_when(ResourceActionWhen::new().with_extensions([".png"])),
                    ]),
            ],
        };

        assert_eq!(
            registry.detect_content_kind(None, Some("images/demo.png")),
            Some(ResourceKind::from("core:image"))
        );
    }
}
