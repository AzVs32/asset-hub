use crate::config::{KindRegistryConfig, ResourceKindConfig};
use asset_core::CoreError;
use asset_core::domain::ResourceKind;
use asset_core::port::{ResourceKindDefinition, ResourceKindRegistry};
use serde::Deserialize;
use std::path::{Path, PathBuf};

const OFFICIAL_PLUGIN_MANIFESTS: &[&str] = &[include_str!("../../plugins/core-book.json")];

/// 默认内置资源类型注册表。
///
/// 当前用于 MVP 阶段。后续插件系统接入后，可以替换为聚合插件定义的 registry 实现。
#[derive(Debug, Clone)]
pub struct DefaultResourceKindRegistry {
    definitions: Vec<ResourceKindDefinition>,
}

impl DefaultResourceKindRegistry {
    /// 创建默认内置注册表。
    pub fn new() -> Result<Self, CoreError> {
        Self::from_config(&KindRegistryConfig::default())
    }

    /// 从配置和插件 manifest 创建资源类型注册表。
    pub fn from_config(config: &KindRegistryConfig) -> Result<Self, CoreError> {
        let mut definitions = Vec::new();

        for kind in ResourceKind::builtin_values() {
            push_definition(
                &mut definitions,
                definition_from_parts(kind, kind, None, None, true, Vec::new(), "builtin")?,
            )?;
        }

        for manifest in load_official_plugin_manifests()? {
            for config_definition in &manifest.resource_kinds {
                push_definition(
                    &mut definitions,
                    definition_from_config(
                        config_definition,
                        format!("plugin:{}", manifest.plugin_id),
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

        for manifest_dir in &config.plugin_manifest_dirs {
            for manifest in load_plugin_manifests(manifest_dir)? {
                for config_definition in &manifest.resource_kinds {
                    push_definition(
                        &mut definitions,
                        definition_from_config(
                            config_definition,
                            format!("plugin:{}", manifest.plugin_id),
                        )?,
                    )?;
                }
            }
        }

        Ok(Self { definitions })
    }
}

impl Default for DefaultResourceKindRegistry {
    fn default() -> Self {
        Self::new().expect("default resource kind definitions should be valid")
    }
}

impl ResourceKindRegistry for DefaultResourceKindRegistry {
    fn list(&self) -> Vec<ResourceKindDefinition> {
        self.definitions.clone()
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

fn definition_from_config(
    config: &ResourceKindConfig,
    source: impl Into<String>,
) -> Result<ResourceKindDefinition, CoreError> {
    let label = config.label.as_deref().unwrap_or(config.kind.as_str());
    definition_from_parts(
        &config.kind,
        label,
        config.schema_id.clone(),
        config.metadata_schema.clone(),
        config.supports_content,
        config.capabilities.clone(),
        source,
    )
}

fn definition_from_parts(
    kind: &str,
    label: &str,
    schema_id: Option<String>,
    metadata_schema: Option<serde_json::Value>,
    supports_content: bool,
    capabilities: Vec<String>,
    source: impl Into<String>,
) -> Result<ResourceKindDefinition, CoreError> {
    Ok(ResourceKindDefinition::with_source(
        ResourceKind::try_new(kind)?,
        label,
        schema_id,
        supports_content,
        source,
    )
    .with_metadata_schema(metadata_schema)
    .with_capabilities(capabilities))
}

fn load_official_plugin_manifests() -> Result<Vec<PluginManifest>, CoreError> {
    OFFICIAL_PLUGIN_MANIFESTS
        .iter()
        .map(|content| {
            serde_json::from_str(content).map_err(|error| {
                CoreError::configuration(format!("parse official plugin manifest: {error}"))
            })
        })
        .collect()
}

fn load_plugin_manifests(path: &Path) -> Result<Vec<PluginManifest>, CoreError> {
    let mut files = std::fs::read_dir(path)
        .map_err(|error| CoreError::configuration(format!("read plugin manifest dir: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<PathBuf>, _>>()
        .map_err(|error| CoreError::configuration(format!("read plugin manifest dir: {error}")))?;
    files.sort();

    files
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(load_plugin_manifest)
        .collect()
}

fn load_plugin_manifest(path: PathBuf) -> Result<PluginManifest, CoreError> {
    let content = std::fs::read_to_string(&path)
        .map_err(|error| CoreError::configuration(format!("read plugin manifest: {error}")))?;
    serde_json::from_str(&content)
        .map_err(|error| CoreError::configuration(format!("parse plugin manifest: {error}")))
}

#[derive(Debug, Deserialize)]
struct PluginManifest {
    plugin_id: String,
    #[serde(default)]
    resource_kinds: Vec<ResourceKindConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn registry_includes_builtin_and_configured_kinds() {
        let registry = DefaultResourceKindRegistry::from_config(&KindRegistryConfig {
            definitions: vec![ResourceKindConfig {
                kind: "doc:note".to_string(),
                label: Some("Note".to_string()),
                schema_id: Some("doc:note@1".to_string()),
                metadata_schema: Some(json!({"type": "object"})),
                supports_content: false,
                capabilities: Vec::new(),
            }],
            plugin_manifest_dirs: Vec::new(),
        })
        .unwrap();

        let builtin = registry
            .list()
            .into_iter()
            .find(|definition| definition.kind().is(ResourceKind::UNKNOWN))
            .unwrap();
        assert_eq!(builtin.source(), "builtin");

        let note = registry
            .get(&ResourceKind::try_new("doc:note").unwrap())
            .unwrap();
        assert_eq!(note.label(), "Note");
        assert_eq!(note.schema_id(), Some("doc:note@1"));
        assert_eq!(note.metadata_schema().unwrap()["type"], "object");
        assert!(!note.supports_content());
        assert!(note.capabilities().is_empty());
        assert_eq!(note.source(), "config");
    }

    #[test]
    fn registry_includes_official_core_book_plugin() {
        let registry = DefaultResourceKindRegistry::new().unwrap();
        let book = registry
            .get(&ResourceKind::try_new("core:book").unwrap())
            .unwrap();

        assert_eq!(book.label(), "Book");
        assert_eq!(book.source(), "plugin:core-book");
        assert!(book.supports_content());
        assert!(book.has_capability("reader"));
    }

    #[test]
    fn registry_loads_plugin_manifest_kinds() {
        let root = unique_temp_path("plugin-manifest");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("mindustry.json"),
            r#"
            {
              "plugin_id": "mindustry",
              "resource_kinds": [
                {
                  "kind": "mindustry:mod",
                  "label": "Mindustry Mod",
                  "schema_id": "mindustry:mod@1",
                  "supports_content": true,
                  "capabilities": ["preview"],
                  "metadata_schema": {
                    "type": "object"
                  }
                }
              ]
            }
            "#,
        )
        .unwrap();

        let registry = DefaultResourceKindRegistry::from_config(&KindRegistryConfig {
            definitions: Vec::new(),
            plugin_manifest_dirs: vec![root.clone()],
        })
        .unwrap();
        let definition = registry
            .get(&ResourceKind::try_new("mindustry:mod").unwrap())
            .unwrap();

        assert_eq!(definition.label(), "Mindustry Mod");
        assert_eq!(definition.source(), "plugin:mindustry");
        assert!(definition.has_capability("preview"));
        assert_eq!(definition.metadata_schema().unwrap()["type"], "object");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn registry_rejects_duplicate_kinds() {
        let error = DefaultResourceKindRegistry::from_config(&KindRegistryConfig {
            definitions: vec![ResourceKindConfig {
                kind: ResourceKind::UNKNOWN.to_string(),
                ..ResourceKindConfig::default()
            }],
            plugin_manifest_dirs: Vec::new(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("duplicate resource kind"));
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("asset-hub-kind-{name}-{}", uuid::Uuid::now_v7()))
    }
}
