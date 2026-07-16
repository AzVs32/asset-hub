use crate::config::{KindRegistryConfig, ResourceKindConfig};
use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::ResourceKind;
use asset_core::port::{ResourceActionRegistry, ResourceKindDefinition, ResourceKindRegistry};
use asset_plugin_api::{
    PluginRuntime, ResourceActionCapability, ResourceActionDefinition, ResourceContentMatcher,
    ResourceKindCapability,
};
use std::collections::{HashMap, HashSet};

/// 默认内置资源类型注册表。
///
/// 当前用于 MVP 阶段。后续插件系统接入后，可以替换为聚合插件定义的 registry 实现。
#[derive(Debug, Clone)]
pub struct DefaultResourceKindRegistry {
    definitions: Vec<ResourceKindDefinition>,
    indices: HashMap<ResourceKind, usize>,
    lineages: HashMap<ResourceKind, Vec<ResourceKind>>,
    descendants: HashMap<ResourceKind, Vec<ResourceKind>>,
}

/// 默认资源动作注册表。
#[derive(Debug, Clone)]
pub struct DefaultResourceActionRegistry {
    actions: Vec<ResourceActionDefinition>,
}

impl DefaultResourceKindRegistry {
    /// 创建默认内置注册表。
    pub fn new() -> Result<Self, CoreError> {
        Self::from_config(&KindRegistryConfig::default())
    }

    /// 从配置和插件 manifest 创建资源类型注册表。
    pub fn from_config(config: &KindRegistryConfig) -> Result<Self, CoreError> {
        let (definitions, _) = build_registries(config)?;
        Ok(Self::from_definitions(definitions))
    }

    fn from_definitions(definitions: Vec<ResourceKindDefinition>) -> Self {
        let indices = definitions
            .iter()
            .enumerate()
            .map(|(index, definition)| (definition.kind().clone(), index))
            .collect::<HashMap<_, _>>();
        let mut lineages = HashMap::with_capacity(definitions.len());
        for definition in &definitions {
            let mut lineage = Vec::new();
            let mut current = Some(definition.kind());
            while let Some(kind) = current {
                lineage.push(kind.clone());
                current = indices
                    .get(kind)
                    .and_then(|index| definitions[*index].parent());
            }
            lineages.insert(definition.kind().clone(), lineage);
        }
        let mut descendants = definitions
            .iter()
            .map(|definition| (definition.kind().clone(), Vec::new()))
            .collect::<HashMap<_, _>>();
        for definition in &definitions {
            let kind = definition.kind();
            let lineage = &lineages[kind];
            for ancestor in lineage {
                descendants
                    .get_mut(ancestor)
                    .expect("lineage kinds must be indexed")
                    .push(kind.clone());
            }
        }

        Self {
            definitions,
            indices,
            lineages,
            descendants,
        }
    }
}

impl DefaultResourceActionRegistry {
    /// 创建默认资源动作注册表。
    pub fn new() -> Result<Self, CoreError> {
        Self::from_config(&KindRegistryConfig::default())
    }

    /// 从配置和插件 manifest 创建资源动作注册表。
    pub fn from_config(config: &KindRegistryConfig) -> Result<Self, CoreError> {
        let (_, actions) = build_registries(config)?;
        Ok(Self { actions })
    }
}

pub(crate) fn registries_from_catalog(
    config: &KindRegistryConfig,
    catalog: &PluginCatalog,
) -> Result<(DefaultResourceKindRegistry, DefaultResourceActionRegistry), CoreError> {
    let (definitions, actions) = build_registries_with_catalog(config, catalog)?;
    Ok((
        DefaultResourceKindRegistry::from_definitions(definitions),
        DefaultResourceActionRegistry { actions },
    ))
}

fn build_registries(
    config: &KindRegistryConfig,
) -> Result<(Vec<ResourceKindDefinition>, Vec<ResourceActionDefinition>), CoreError> {
    let catalog = PluginCatalog::load(config)?;
    build_registries_with_catalog(config, &catalog)
}

fn build_registries_with_catalog(
    config: &KindRegistryConfig,
    catalog: &PluginCatalog,
) -> Result<(Vec<ResourceKindDefinition>, Vec<ResourceActionDefinition>), CoreError> {
    let mut definitions = Vec::new();
    let official_manifests = catalog
        .plugins()
        .iter()
        .filter(|plugin| plugin.manifest_path.is_none())
        .collect::<Vec<_>>();
    let plugin_manifests = catalog
        .plugins()
        .iter()
        .filter(|plugin| plugin.manifest_path.is_some())
        .collect::<Vec<_>>();

    for kind in ResourceKind::builtin_values() {
        let parent = (*kind == ResourceKind::UNKNOWN).then_some("core:file");
        push_definition(
            &mut definitions,
            definition_from_parts(
                kind,
                kind,
                parent,
                true,
                ResourceContentMatcher::default(),
                "builtin",
            )?,
        )?;
    }
    for manifest in &official_manifests {
        for config_definition in &manifest.manifest.capabilities.resource_kinds {
            push_definition(
                &mut definitions,
                definition_from_manifest_kind(
                    config_definition,
                    format!("plugin:{}", manifest.manifest.plugin_id()),
                )?,
            )?;
        }
    }

    for config_definition in &config.definitions {
        push_definition(
            &mut definitions,
            definition_from_config(config_definition, "config")?,
        )?;
    }

    for manifest in &plugin_manifests {
        for config_definition in &manifest.manifest.capabilities.resource_kinds {
            push_definition(
                &mut definitions,
                definition_from_manifest_kind(
                    config_definition,
                    format!("plugin:{}", manifest.manifest.plugin_id()),
                )?,
            )?;
        }
    }
    validate_kind_hierarchy(&definitions)?;

    let mut actions = Vec::new();
    for definition in &config.definitions {
        for action in &definition.actions {
            push_action_definition(
                &mut actions,
                action.clone().with_kinds([definition.kind.clone()]),
                format!("config kind `{}`", definition.kind),
            )?;
        }
    }
    for manifest in &official_manifests {
        for action in &manifest.manifest.capabilities.resource_actions {
            let action_definitions = action_definitions_with_inherited_content(
                &definitions,
                action,
                &manifest.manifest.runtime,
            )?;
            for action_definition in action_definitions {
                push_action_definition(
                    &mut actions,
                    action_definition,
                    format!("plugin:{}", manifest.manifest.plugin_id()),
                )?;
            }
        }
    }
    for manifest in &plugin_manifests {
        for action in &manifest.manifest.capabilities.resource_actions {
            let action_definitions = action_definitions_with_inherited_content(
                &definitions,
                action,
                &manifest.manifest.runtime,
            )?;
            for action_definition in action_definitions {
                push_action_definition(
                    &mut actions,
                    action_definition,
                    format!("plugin:{}", manifest.manifest.plugin_id()),
                )?;
            }
        }
    }

    Ok((definitions, actions))
}

fn validate_kind_hierarchy(definitions: &[ResourceKindDefinition]) -> Result<(), CoreError> {
    let parents = definitions
        .iter()
        .map(|definition| {
            (
                definition.kind().as_str(),
                definition.parent().map(|parent| parent.as_str()),
            )
        })
        .collect::<HashMap<_, _>>();

    for definition in definitions {
        let mut current = Some(definition.kind().as_str());
        let mut visited = HashSet::new();
        while let Some(kind) = current {
            if !visited.insert(kind) {
                return Err(CoreError::configuration(format!(
                    "resource kind hierarchy contains a cycle at `{kind}`"
                )));
            }
            let Some(parent) = parents.get(kind) else {
                return Err(CoreError::configuration(format!(
                    "resource kind `{}` references unknown parent `{kind}`",
                    definition.kind()
                )));
            };
            current = *parent;
        }
    }
    Ok(())
}

impl Default for DefaultResourceKindRegistry {
    fn default() -> Self {
        Self::new().expect("default resource kind definitions should be valid")
    }
}

impl Default for DefaultResourceActionRegistry {
    fn default() -> Self {
        Self::new().expect("default resource action definitions should be valid")
    }
}

impl ResourceKindRegistry for DefaultResourceKindRegistry {
    fn definitions(&self) -> &[ResourceKindDefinition] {
        &self.definitions
    }

    fn get(&self, kind: &ResourceKind) -> Option<&ResourceKindDefinition> {
        self.indices
            .get(kind)
            .map(|index| &self.definitions[*index])
    }

    fn lineage(&self, kind: &ResourceKind) -> Vec<ResourceKind> {
        self.lineages.get(kind).cloned().unwrap_or_default()
    }

    fn descendants(&self, kind: &ResourceKind) -> Vec<ResourceKind> {
        self.descendants.get(kind).cloned().unwrap_or_default()
    }
}

impl ResourceActionRegistry for DefaultResourceActionRegistry {
    fn actions(&self) -> &[ResourceActionDefinition] {
        &self.actions
    }
}

fn push_definition(
    definitions: &mut Vec<ResourceKindDefinition>,
    definition: ResourceKindDefinition,
) -> Result<(), CoreError> {
    if definitions
        .iter()
        .any(|existing| existing.kind().as_str() == definition.kind().as_str())
    {
        return Err(CoreError::configuration(format!(
            "duplicate resource kind `{}`",
            definition.kind()
        )));
    }

    definitions.push(definition);
    Ok(())
}

fn action_definitions_with_inherited_content(
    definitions: &[ResourceKindDefinition],
    action: &ResourceActionCapability,
    runtime: &PluginRuntime,
) -> Result<Vec<ResourceActionDefinition>, CoreError> {
    let definition = action.to_definition(runtime);
    if !should_inherit_detect_for_action(action) || !definition.content_matcher().is_empty() {
        return Ok(vec![definition]);
    }
    action
        .applies_to
        .kinds
        .iter()
        .map(|kind| {
            Ok(definition
                .clone()
                .with_kinds([kind.clone()])
                .with_content_matcher(detect_for_kind(definitions, kind)?))
        })
        .collect()
}

fn should_inherit_detect_for_action(action: &ResourceActionCapability) -> bool {
    action.applies_to.kinds.len() > 1
}

fn detect_for_kind(
    definitions: &[ResourceKindDefinition],
    kind: &str,
) -> Result<ResourceContentMatcher, CoreError> {
    definitions
        .iter()
        .find(|definition| definition.kind().as_str().eq_ignore_ascii_case(kind))
        .map(|definition| definition.detect().clone())
        .ok_or_else(|| {
            CoreError::configuration(format!("resource action references unknown kind `{kind}`"))
        })
}

fn push_action_definition(
    actions: &mut Vec<ResourceActionDefinition>,
    action: ResourceActionDefinition,
    source: impl Into<String>,
) -> Result<(), CoreError> {
    let source = source.into();
    if actions.iter().any(|existing| {
        existing.id().as_str() == action.id().as_str()
            && (existing.kinds().is_empty()
                || action.kinds().is_empty()
                || existing.kinds().iter().any(|kind| {
                    action
                        .kinds()
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(kind))
                }))
    }) {
        return Err(CoreError::configuration(format!(
            "duplicate global resource action `{}` from {source}",
            action.id()
        )));
    }

    actions.push(action);
    Ok(())
}

fn definition_from_manifest_kind(
    config: &ResourceKindCapability,
    source: impl Into<String>,
) -> Result<ResourceKindDefinition, CoreError> {
    let label = config.label.as_deref().unwrap_or(config.kind.as_str());
    definition_from_parts(
        &config.kind,
        label,
        config.parent.as_deref(),
        config.supports_content,
        config.detect.clone(),
        source,
    )
}

fn definition_from_config(
    config: &ResourceKindConfig,
    source: impl Into<String>,
) -> Result<ResourceKindDefinition, CoreError> {
    let label = config.label.as_deref().unwrap_or(config.kind.as_str());
    definition_from_parts(
        &config.kind,
        label,
        config.parent.as_deref(),
        config.supports_content,
        config.detect.clone(),
        source,
    )
}

fn definition_from_parts(
    kind: &str,
    label: &str,
    parent: Option<&str>,
    supports_content: bool,
    detect: ResourceContentMatcher,
    source: impl Into<String>,
) -> Result<ResourceKindDefinition, CoreError> {
    Ok(ResourceKindDefinition::with_source(
        ResourceKind::try_new(kind)?,
        label,
        supports_content,
        source,
    )
    .with_parent(parent.map(ResourceKind::try_new).transpose()?)
    .with_detect(detect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_plugin_api::ResourceAction;
    use std::path::PathBuf;

    #[test]
    fn descendants_follow_definition_order() {
        let registry = DefaultResourceKindRegistry::new().unwrap();
        let root = ResourceKind::try_new("core:file").unwrap();
        let expected = registry
            .definitions
            .iter()
            .filter(|definition| registry.lineages[definition.kind()].contains(&root))
            .map(|definition| definition.kind().clone())
            .collect::<Vec<_>>();
        assert_eq!(registry.descendants(&root), expected);
    }

    #[test]
    fn registry_includes_builtin_and_configured_kinds() {
        let registry = DefaultResourceKindRegistry::from_config(&KindRegistryConfig {
            definitions: vec![ResourceKindConfig {
                kind: "doc:note".to_string(),
                label: Some("Note".to_string()),
                supports_content: false,
                actions: Vec::new(),
                ..ResourceKindConfig::default()
            }],
            plugin_manifests: Vec::new(),
        })
        .unwrap();

        let builtin = registry
            .definitions()
            .iter()
            .find(|definition| definition.kind().is(ResourceKind::UNKNOWN))
            .unwrap();
        assert_eq!(builtin.source(), "builtin");

        let note = registry
            .get(&ResourceKind::try_new("doc:note").unwrap())
            .unwrap();
        assert_eq!(note.label(), "Note");
        assert!(!note.supports_content());
        assert_eq!(note.source(), "config");
    }

    #[test]
    fn registry_rejects_unknown_parents_and_cycles() {
        let unknown_parent = KindRegistryConfig {
            definitions: vec![ResourceKindConfig {
                kind: "code:c".to_string(),
                parent: Some("core:missing".to_string()),
                ..ResourceKindConfig::default()
            }],
            plugin_manifests: Vec::new(),
        };
        assert!(
            DefaultResourceKindRegistry::from_config(&unknown_parent)
                .unwrap_err()
                .to_string()
                .contains("unknown parent")
        );

        let cycle = KindRegistryConfig {
            definitions: vec![
                ResourceKindConfig {
                    kind: "code:a".to_string(),
                    parent: Some("code:b".to_string()),
                    ..ResourceKindConfig::default()
                },
                ResourceKindConfig {
                    kind: "code:b".to_string(),
                    parent: Some("code:a".to_string()),
                    ..ResourceKindConfig::default()
                },
            ],
            plugin_manifests: Vec::new(),
        };
        assert!(
            DefaultResourceKindRegistry::from_config(&cycle)
                .unwrap_err()
                .to_string()
                .contains("cycle")
        );
    }

    #[test]
    fn registry_includes_official_core_plugin_fallback_kinds() {
        let registry = DefaultResourceKindRegistry::new().unwrap();
        let action_registry = DefaultResourceActionRegistry::new().unwrap();

        let file = registry
            .get(&ResourceKind::try_new("core:file").unwrap())
            .unwrap();
        let unknown = registry
            .get(&ResourceKind::try_new(ResourceKind::UNKNOWN).unwrap())
            .unwrap();
        assert!(file.parent().is_none());
        assert_eq!(unknown.parent(), Some(file.kind()));
        assert!(
            actions_for_kind(&registry, &action_registry, unknown.kind())
                .iter()
                .any(|action| action.id().as_str() == ResourceAction::DOWNLOAD_CONTENT)
        );

        for (kind, label, source, expected_actions) in [
            (
                "core:file",
                "File",
                "plugin:core.file",
                vec![ResourceAction::DOWNLOAD_CONTENT],
            ),
            (
                "core:image",
                "Image",
                "plugin:core.image",
                vec![
                    ResourceAction::DOWNLOAD_CONTENT,
                    ResourceAction::VIEW_INLINE,
                    ResourceAction::PREVIEW,
                    ResourceAction::THUMBNAIL,
                ],
            ),
            (
                "core:document",
                "Document",
                "plugin:core.document",
                vec![
                    ResourceAction::DOWNLOAD_CONTENT,
                    ResourceAction::VIEW_INLINE,
                    ResourceAction::PREVIEW,
                ],
            ),
            (
                "core:video",
                "Video",
                "plugin:core.video",
                vec![
                    ResourceAction::DOWNLOAD_CONTENT,
                    ResourceAction::VIEW_INLINE,
                    ResourceAction::PREVIEW,
                ],
            ),
        ] {
            let definition = registry.get(&ResourceKind::try_new(kind).unwrap()).unwrap();

            assert_eq!(definition.label(), label);
            assert_eq!(definition.source(), source);
            assert!(definition.supports_content());
            let inherited_actions =
                actions_for_kind(&registry, &action_registry, definition.kind());
            for action in expected_actions {
                assert!(
                    inherited_actions
                        .iter()
                        .any(|definition| definition.id().as_str() == action)
                );
            }
            assert!(
                !inherited_actions
                    .iter()
                    .any(|definition| definition.id().as_str() == ResourceAction::READ)
            );
        }

        let image = registry
            .get(&ResourceKind::try_new("core:image").unwrap())
            .unwrap();
        assert!(
            image
                .detect()
                .matches_content(Some("image/png"), Some("images/pixel.png"))
        );
        let file = registry
            .get(&ResourceKind::try_new("core:file").unwrap())
            .unwrap();
        assert!(file.detect().is_empty());
    }

    #[test]
    fn registry_exposes_actions_as_global_capabilities() {
        let registry = DefaultResourceActionRegistry::new().unwrap();
        let actions = registry.actions();

        assert!(actions.iter().any(|action| {
            action.id().as_str() == ResourceAction::PREVIEW
                && action.matches_resource("core:video", Some("video/mp4"), Some("demo.mp4"))
        }));
        assert!(actions.iter().any(|action| {
            action.id().as_str() == ResourceAction::PREVIEW
                && action.matches_resource("core:image", Some("image/png"), Some("demo.png"))
        }));
    }

    #[test]
    fn registry_loads_plugin_manifest_kinds() {
        let root = unique_temp_path("plugin-manifest");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("mindustry.json"),
            r#"
            {
              "manifest_version": 2,
              "plugin": {
                "id": "mindustry",
                "name": "Mindustry",
                "version": "0.1.0",
                "publisher": "test",
                "description": "Mindustry test plugin."
              },
              "runtime": {
                "type": "builtin"
              },
              "capabilities": {
                "resource_kinds": [
                  {
                    "kind": "mindustry:mod",
                    "label": "Mindustry Mod",
                    "supports_content": true
                  }
                ],
                "resource_actions": [
                  {
                    "id": "mindustry.preview",
                    "label": "Preview",
                    "handler": "builtin.media.preview",
                    "applies_to": {
                      "kinds": ["mindustry:mod"]
                    },
                    "access": "read",
                    "views": ["media"]
                  }
                ]
              },
              "permissions": {
                "resource": {
                  "read": true,
                  "write": false
                },
                "content": {
                  "read": true,
                  "write": false
                },
                "network": false,
                "filesystem": false
              }
            }
            "#,
        )
        .unwrap();

        let config = KindRegistryConfig {
            definitions: Vec::new(),
            plugin_manifests: vec![root.join("mindustry.json")],
        };
        let registry = DefaultResourceKindRegistry::from_config(&config).unwrap();
        let action_registry = DefaultResourceActionRegistry::from_config(&config).unwrap();
        let definition = registry
            .get(&ResourceKind::try_new("mindustry:mod").unwrap())
            .unwrap();

        assert_eq!(definition.label(), "Mindustry Mod");
        assert_eq!(definition.source(), "plugin:mindustry");
        assert!(
            actions_for_kind(&registry, &action_registry, definition.kind())
                .iter()
                .any(|action| action.id().as_str() == "mindustry.preview")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn registry_loads_format_plugin_as_independent_kind() {
        let root = unique_temp_path("plugin-kind");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("epub.wasm"), []).unwrap();
        std::fs::write(
            root.join("epub.json"),
            r#"
            {
              "manifest_version": 2,
              "plugin": {
                "id": "epub",
                "name": "EPUB",
                "version": "0.1.0",
                "publisher": "test",
                "description": "EPUB test plugin."
              },
              "runtime": {
                "type": "extism",
                "wasm": "epub.wasm",
                "wasi": false,
                "plugin_api": "asset-hub.plugin-api@0.1"
              },
              "capabilities": {
                "resource_kinds": [
                  {
                    "kind": "azvs:epub",
                    "label": "EPUB",
                    "supports_content": true,
                    "detect": {
                      "mime_types": ["application/epub+zip"],
                      "extensions": [".epub"]
                    }
                  }
                ],
                "resource_actions": [
                  {
                    "id": "azvs.epub.render",
                    "label": "Read EPUB",
                    "handler": "render_epub",
                    "applies_to": {
                      "kinds": ["azvs:epub"]
                    },
                    "access": "read",
                    "views": ["html"]
                  }
                ]
              },
              "permissions": {
                "resource": {
                  "read": true,
                  "write": false
                },
                "content": {
                  "read": true,
                  "write": false
                },
                "network": false,
                "filesystem": false
              }
            }
            "#,
        )
        .unwrap();
        write_empty_wasm_lock(&root, "epub");

        let config = KindRegistryConfig {
            definitions: Vec::new(),
            plugin_manifests: vec![root.join("epub.json")],
        };
        let registry = DefaultResourceKindRegistry::from_config(&config).unwrap();
        let action_registry = DefaultResourceActionRegistry::from_config(&config).unwrap();
        let epub = registry
            .get(&ResourceKind::try_new("azvs:epub").unwrap())
            .unwrap();

        assert_eq!(epub.label(), "EPUB");
        assert_eq!(epub.source(), "plugin:epub");
        assert!(
            actions_for_kind(&registry, &action_registry, epub.kind())
                .iter()
                .any(|action| action.id().as_str() == "azvs.epub.render")
        );
        assert!(
            epub.detect()
                .matches_content(Some("application/epub+zip"), Some("books/book.epub"))
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn registry_loads_plugin_manifest_kind_extensions() {
        let root = unique_temp_path("plugin-extension");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("mp4-tools.wasm"), []).unwrap();
        std::fs::write(
            root.join("mp4-tools.json"),
            r#"
            {
              "manifest_version": 2,
              "plugin": {
                "id": "mp4-tools",
                "name": "MP4 Tools",
                "version": "0.1.0",
                "publisher": "test",
                "description": "MP4 test plugin."
              },
              "runtime": {
                "type": "extism",
                "wasm": "mp4-tools.wasm",
                "wasi": false,
                "plugin_api": "asset-hub.plugin-api@0.1"
              },
              "capabilities": {
                "resource_kinds": [
                  {
                    "kind": "test:mp4",
                    "parent": "core:video",
                    "label": "MP4",
                    "supports_content": true,
                    "detect": {
                      "mime_types": ["video/mp4"],
                      "extensions": [".mp4"]
                    }
                  }
                ],
                "resource_actions": [
                  {
                    "id": "mp4-tools:inspect",
                    "label": "Inspect MP4",
                    "handler": "inspect_mp4",
                    "applies_to": {
                      "kinds": ["test:mp4"]
                    },
                    "access": "read",
                    "views": ["json"]
                  }
                ]
              },
              "permissions": {
                "resource": {
                  "read": true,
                  "write": false
                },
                "content": {
                  "read": true,
                  "write": false
                },
                "network": false,
                "filesystem": false
              }
            }
            "#,
        )
        .unwrap();
        write_empty_wasm_lock(&root, "mp4-tools");

        let config = KindRegistryConfig {
            definitions: Vec::new(),
            plugin_manifests: vec![root.join("mp4-tools.json")],
        };
        let registry = DefaultResourceKindRegistry::from_config(&config).unwrap();
        let action_registry = DefaultResourceActionRegistry::from_config(&config).unwrap();
        let video = registry
            .get(&ResourceKind::try_new("test:mp4").unwrap())
            .unwrap();
        let action = actions_for_kind(&registry, &action_registry, video.kind())
            .into_iter()
            .find(|action| action.id().as_str() == "mp4-tools:inspect")
            .unwrap();

        assert_eq!(action.handler(), Some("inspect_mp4"));
        assert!(action.content_matcher().is_empty());
        assert!(action.matches_resource(
            "test:mp4",
            Some("video/webm"),
            Some("videos/internal-object-key")
        ));
        assert!(!action.matches_resource("core:video", Some("video/mp4"), Some("videos/demo.mp4")));

        let _ = std::fs::remove_dir_all(root);
    }

    fn actions_for_kind(
        kinds: &dyn ResourceKindRegistry,
        actions: &dyn ResourceActionRegistry,
        kind: &ResourceKind,
    ) -> Vec<ResourceActionDefinition> {
        actions.actions_for_kinds(&kinds.lineage(kind))
    }

    #[test]
    fn registry_rejects_duplicate_kinds() {
        let error = DefaultResourceKindRegistry::from_config(&KindRegistryConfig {
            definitions: vec![ResourceKindConfig {
                kind: ResourceKind::UNKNOWN.to_string(),
                ..ResourceKindConfig::default()
            }],
            plugin_manifests: Vec::new(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("duplicate resource kind"));
    }

    #[test]
    fn registry_rejects_duplicate_global_action_ids() {
        let root = unique_temp_path("duplicate-action");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("duplicate-preview.json"),
            r#"
            {
              "manifest_version": 2,
              "plugin": {
                "id": "duplicate-preview",
                "name": "Duplicate Preview",
                "version": "0.1.0",
                "publisher": "test",
                "description": "Duplicate action id test plugin."
              },
              "runtime": {
                "type": "builtin"
              },
              "capabilities": {
                "resource_kinds": [],
                "resource_actions": [
                  {
                    "id": "preview",
                    "label": "Preview",
                    "handler": "builtin.media.preview",
                    "applies_to": {
                      "kinds": ["core:image"]
                    },
                    "access": "read",
                    "views": ["media"]
                  }
                ]
              },
              "permissions": {
                "resource": {
                  "read": true,
                  "write": false
                },
                "content": {
                  "read": true,
                  "write": false
                },
                "network": false,
                "filesystem": false
              }
            }
            "#,
        )
        .unwrap();

        let error = DefaultResourceActionRegistry::from_config(&KindRegistryConfig {
            definitions: Vec::new(),
            plugin_manifests: vec![root.join("duplicate-preview.json")],
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("duplicate global resource action `preview`")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn write_empty_wasm_lock(root: &std::path::Path, plugin_id: &str) {
        std::fs::write(
            root.join("manifest.lock.json"),
            format!(
                r#"{{
                  "manifest_version": 2,
                  "plugin_id": "{plugin_id}",
                  "runtime": {{
                    "wasm_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                  }}
                }}"#
            ),
        )
        .unwrap();
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("asset-hub-kind-{name}-{}", uuid::Uuid::now_v7()))
    }
}
