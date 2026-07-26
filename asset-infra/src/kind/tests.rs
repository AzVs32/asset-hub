use super::*;
use crate::config::{KindRegistryConfig, ResourceKindConfig};
use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::{DirectoryKind, ResourceKind};
use asset_core::port::{DirectoryKindRegistry, ResourceActionRegistry, ResourceKindRegistry};
use asset_plugin_api::{ResourceAction, ResourceActionDefinition};
use std::path::PathBuf;

fn registries(
    config: &KindRegistryConfig,
) -> Result<
    (
        DefaultResourceKindRegistry,
        DefaultDirectoryKindRegistry,
        DefaultResourceActionRegistry,
    ),
    CoreError,
> {
    let catalog = PluginCatalog::load(config)?;
    registries_from_catalog(config, &catalog)
}

fn kind_registry(config: &KindRegistryConfig) -> Result<DefaultResourceKindRegistry, CoreError> {
    registries(config).map(|(resource_kinds, _, _)| resource_kinds)
}

fn action_registry(
    config: &KindRegistryConfig,
) -> Result<DefaultResourceActionRegistry, CoreError> {
    registries(config).map(|(_, _, actions)| actions)
}

#[test]
fn descendants_follow_definition_order() {
    let registry = kind_registry(&KindRegistryConfig::default()).unwrap();
    let root = ResourceKind::try_new("core:resource").unwrap();
    let expected = registry
        .definitions
        .iter()
        .filter(|definition| registry.lineages[definition.kind()].contains(&root))
        .map(|definition| definition.kind().clone())
        .collect::<Vec<_>>();
    assert_eq!(registry.descendants(&root), expected);
}

#[test]
fn registry_includes_official_and_configured_kinds() {
    let registry = kind_registry(&KindRegistryConfig {
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

    let default = registry
        .definitions()
        .iter()
        .find(|definition| definition.kind().is(ResourceKind::DEFAULT))
        .unwrap();
    assert_eq!(default.source(), "plugin:core.resource");

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
        kind_registry(&unknown_parent)
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
        kind_registry(&cycle)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
}

#[test]
fn registry_includes_official_core_resource_kinds() {
    let (registry, _, action_registry) = registries(&KindRegistryConfig::default()).unwrap();

    let root = registry
        .get(&ResourceKind::try_new("core:resource").unwrap())
        .unwrap();
    assert!(root.parent().is_none());
    assert!(
        actions_for_kind(&registry, &action_registry, root.kind())
            .iter()
            .any(|action| action.id().as_str() == ResourceAction::CORE_RESOURCE_DOWNLOAD)
    );

    for (kind, label, source, expected_actions) in [
        (
            "core:resource",
            "Resource",
            "plugin:core.resource",
            vec![ResourceAction::CORE_RESOURCE_DOWNLOAD],
        ),
        (
            "core:image",
            "Image",
            "plugin:core.image",
            vec![ResourceAction::CORE_RESOURCE_DOWNLOAD],
        ),
        (
            "core:document",
            "Document",
            "plugin:core.document",
            vec![ResourceAction::CORE_RESOURCE_DOWNLOAD],
        ),
        (
            "core:video",
            "Video",
            "plugin:core.video",
            vec![ResourceAction::CORE_RESOURCE_DOWNLOAD],
        ),
    ] {
        let definition = registry.get(&ResourceKind::try_new(kind).unwrap()).unwrap();

        assert_eq!(definition.label(), label);
        assert_eq!(definition.source(), source);
        assert!(definition.supports_content());
        let inherited_actions = actions_for_kind(&registry, &action_registry, definition.kind());
        for action in expected_actions {
            assert!(
                inherited_actions
                    .iter()
                    .any(|definition| definition.id().as_str() == action)
            );
        }
        assert_eq!(inherited_actions.len(), 1);
    }

    let image = registry
        .get(&ResourceKind::try_new("core:image").unwrap())
        .unwrap();
    assert!(
        image
            .detect()
            .matches_content(Some("image/png"), Some("images/pixel.png"))
    );
    let resource = registry
        .get(&ResourceKind::try_new("core:resource").unwrap())
        .unwrap();
    assert!(resource.detect().is_empty());
}

#[test]
fn registry_exposes_actions_as_global_capabilities() {
    let registry = action_registry(&KindRegistryConfig::default()).unwrap();
    let actions = registry.actions();

    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].id().as_str(),
        ResourceAction::CORE_RESOURCE_DOWNLOAD
    );
}

#[test]
fn registry_loads_plugin_manifest_kinds() {
    let root = unique_temp_path("plugin-manifest");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("mindustry.json"),
        r#"
        {
          "manifest_version": 3,
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
            "kinds": [
              {
                "kind": "mindustry:mod",
                "label": "Mindustry Mod",
                "supports_content": true
              }
            ],
            "directory_kinds": [
              {
                "kind": "mindustry:workspace",
                "parent": "core:directory",
                "label": "Mindustry Workspace"
              }
            ],
            "actions": [
              {
                "id": "mindustry.download",
                "label": "Download Mod",
                "handler": "builtin.resource.download",
                "applies_to": {
                  "kinds": ["mindustry:mod"]
                },
                "access": "read",
                "views": ["download"]
              }
            ]
          },
          "permissions": {
            "allow": ["resource.read", "content.read"],
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
    let (registry, directory_registry, action_registry) = registries(&config).unwrap();
    let definition = registry
        .get(&ResourceKind::try_new("mindustry:mod").unwrap())
        .unwrap();

    assert_eq!(definition.label(), "Mindustry Mod");
    assert_eq!(definition.source(), "plugin:mindustry");
    let directory_definition = directory_registry
        .get(&DirectoryKind::try_new("mindustry:workspace").unwrap())
        .unwrap();
    assert_eq!(directory_definition.label(), "Mindustry Workspace");
    assert_eq!(
        directory_definition.parent(),
        Some(&DirectoryKind::default())
    );
    assert_eq!(directory_definition.source(), "plugin:mindustry");
    assert!(
        actions_for_kind(&registry, &action_registry, definition.kind())
            .iter()
            .any(|action| action.id().as_str() == "mindustry.download")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn directory_registry_includes_official_default_kind() {
    let (_, registry, _) = registries(&KindRegistryConfig::default()).unwrap();
    let default = DirectoryKind::default();
    let definition = registry.get(&default).unwrap();

    assert!(definition.parent().is_none());
    assert_eq!(definition.label(), "Directory");
    assert_eq!(definition.source(), "plugin:core.directory");
    assert_eq!(registry.lineage(&default), vec![default]);
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
          "manifest_version": 3,
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
            "plugin_api": "asset-hub.plugin-api@0.4"
          },
          "capabilities": {
            "kinds": [
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
            "actions": [
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
            "allow": ["resource.read", "content.read"],
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
    let (registry, _, action_registry) = registries(&config).unwrap();
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
          "manifest_version": 3,
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
            "plugin_api": "asset-hub.plugin-api@0.4"
          },
          "capabilities": {
            "kinds": [
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
            "actions": [
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
            "allow": ["resource.read", "content.read"],
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
    let (registry, _, action_registry) = registries(&config).unwrap();
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
    let error = kind_registry(&KindRegistryConfig {
        definitions: vec![ResourceKindConfig {
            kind: ResourceKind::DEFAULT.to_string(),
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
        root.join("duplicate-download.json"),
        r#"
        {
          "manifest_version": 3,
          "plugin": {
            "id": "duplicate-download",
            "name": "Duplicate Preview",
            "version": "0.1.0",
            "publisher": "test",
            "description": "Duplicate action id test plugin."
          },
          "runtime": {
            "type": "builtin"
          },
          "capabilities": {
            "kinds": [],
            "actions": [
              {
                "id": "core.resource.download",
                "label": "Duplicate Download",
                "handler": "builtin.resource.download",
                "applies_to": {
                  "kinds": ["core:resource"]
                },
                "access": "read",
                "views": ["download"]
              }
            ]
          },
          "permissions": {
            "allow": ["resource.read", "content.read"],
            "network": false,
            "filesystem": false
          }
        }
        "#,
    )
    .unwrap();

    let error = action_registry(&KindRegistryConfig {
        definitions: Vec::new(),
        plugin_manifests: vec![root.join("duplicate-download.json")],
    })
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("duplicate global resource action `core.resource.download`")
    );

    let _ = std::fs::remove_dir_all(root);
}

fn write_empty_wasm_lock(root: &std::path::Path, plugin_id: &str) {
    std::fs::write(
        root.join("manifest.lock.json"),
        format!(
            r#"{{
              "manifest_version": 3,
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
