use super::*;
use crate::kind::builder::{definition_from_parts, push_definition};
use crate::kind::directory_action_registry::validate_directory_action_capabilities;
use crate::plugin_manifest::PluginCatalog;
use asset_core::CoreError;
use asset_core::domain::{
    ActionOutputContract, ActionUi, DefinitionOrigin, DirectoryActionDefinition, DirectoryKind,
    DirectoryKindDefinition, ResourceActionDefinition, ResourceContentMatcher, ResourceKind,
    ResourceKindDefinition,
};
use asset_core::port::{DirectoryKindRegistry, ResourceActionRegistry, ResourceKindRegistry};
use std::path::{Path, PathBuf};

fn registries(
    packages_root: &Path,
) -> Result<
    (
        DefaultResourceKindRegistry,
        DefaultDirectoryKindRegistry,
        DefaultResourceActionRegistry,
    ),
    CoreError,
> {
    let catalog = PluginCatalog::load(packages_root)?;
    let catalogs = build_capability_catalogs(&catalog)?;
    Ok((
        catalogs.resource_kinds,
        catalogs.directory_kinds,
        catalogs.resource_actions,
    ))
}

fn kind_registry(packages_root: &Path) -> Result<DefaultResourceKindRegistry, CoreError> {
    registries(packages_root).map(|(resource_kinds, _, _)| resource_kinds)
}

fn action_registry(packages_root: &Path) -> Result<DefaultResourceActionRegistry, CoreError> {
    registries(packages_root).map(|(_, _, actions)| actions)
}

#[test]
fn descendants_follow_definition_order() {
    let registry = kind_registry(&unique_temp_path("no-plugins")).unwrap();
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
fn registry_rejects_unknown_parents_and_cycles() {
    let unknown_parent = vec![
        definition_from_parts(
            "code:c",
            "C",
            Some("core:missing"),
            true,
            ResourceContentMatcher::default(),
            DefinitionOrigin::builtin_static("test"),
        )
        .unwrap(),
    ];
    assert!(
        validate_kind_hierarchy(&unknown_parent)
            .unwrap_err()
            .to_string()
            .contains("unknown parent")
    );

    let cycle = vec![
        definition_from_parts(
            "code:a",
            "A",
            Some("code:b"),
            true,
            ResourceContentMatcher::default(),
            DefinitionOrigin::builtin_static("test"),
        )
        .unwrap(),
        definition_from_parts(
            "code:b",
            "B",
            Some("code:a"),
            true,
            ResourceContentMatcher::default(),
            DefinitionOrigin::builtin_static("test"),
        )
        .unwrap(),
    ];
    assert!(
        validate_kind_hierarchy(&cycle)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
}

#[test]
fn registry_includes_host_builtin_resource_kinds() {
    let (registry, _, action_registry) = registries(&unique_temp_path("no-plugins")).unwrap();

    let root = registry
        .get(&ResourceKind::try_new("core:resource").unwrap())
        .unwrap();
    assert!(root.parent().is_none());
    assert!(
        actions_for_kind(&registry, &action_registry, root.kind())
            .iter()
            .any(|action| action.id().as_str() == "core.resource.download")
    );

    for (kind, label, source, expected_actions) in [
        (
            "core:resource",
            "Resource",
            "builtin:core.resource",
            vec!["core.resource.download", "core.resource.thumbnail"],
        ),
        (
            "core:image",
            "Image",
            "builtin:core.image",
            vec!["core.resource.download", "core.image.thumbnail"],
        ),
        (
            "core:text",
            "Text",
            "builtin:core.text",
            vec![
                "core.resource.download",
                "core.resource.thumbnail",
                "core.text.read",
                "core.text.edit",
            ],
        ),
        (
            "core:video",
            "Video",
            "builtin:core.video",
            vec!["core.resource.download", "core.resource.thumbnail"],
        ),
    ] {
        let definition = registry.get(&ResourceKind::try_new(kind).unwrap()).unwrap();

        assert_eq!(definition.label(), label);
        assert_eq!(definition.origin().to_string(), source);
        assert!(definition.supports_content());
        let inherited_actions = actions_for_kind(&registry, &action_registry, definition.kind());
        for action in expected_actions {
            assert!(
                inherited_actions
                    .iter()
                    .any(|definition| definition.id().as_str() == action)
            );
        }
        assert_eq!(
            inherited_actions.len(),
            if kind == "core:text" { 4 } else { 2 }
        );
        if kind == "core:image" {
            assert!(
                inherited_actions
                    .iter()
                    .all(|action| action.id().as_str() != "core.resource.thumbnail")
            );
        }
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
    let text = registry
        .get(&ResourceKind::try_new("core:text").unwrap())
        .unwrap();
    assert!(
        text.detect()
            .matches_content(Some("text/plain; charset=utf-8"), Some("notes/draft.txt"))
    );
    assert!(
        text.detect()
            .matches_content(Some("application/json"), Some("data/settings.json"))
    );
}

#[test]
fn registry_exposes_actions_as_global_capabilities() {
    let registry = action_registry(&unique_temp_path("no-plugins")).unwrap();
    let actions = registry.actions();

    assert_eq!(actions.len(), 5);
    assert_eq!(actions[0].id().as_str(), "core.resource.download");
    assert_eq!(actions[1].id().as_str(), "core.resource.thumbnail");
    assert_eq!(actions[2].id().as_str(), "core.image.thumbnail");
    assert_eq!(
        actions[2].provides().map(|id| id.as_str()),
        Some("thumbnail")
    );
    assert_eq!(actions[3].id().as_str(), "core.text.read");
    assert_eq!(
        actions[3].provides().map(|id| id.as_str()),
        Some("text_read")
    );
    assert_eq!(actions[4].id().as_str(), "core.text.edit");
    assert_eq!(
        actions[4].provides().map(|id| id.as_str()),
        Some("text_edit")
    );
}

#[test]
fn registry_loads_plugin_manifest_kinds() {
    let root = unique_temp_path("plugin-manifest");
    let package = root.join("mindustry");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("plugin.wasm"), []).unwrap();
    std::fs::write(
        package.join("manifest.json"),
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
            "type": "extism",
            "plugin_api": "asset-hub.plugin-api@2"
          },
          "capabilities": {
            "resource_kinds": [
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
            "resource_actions": [
              {
                "id": "mindustry.download",
                "label": "Download Mod",
                "handler": "download_mod",
                "applies_to": {
                  "kinds": ["mindustry:mod"]
                },
                "access": "read",
                "output": {"views": ["download"]}
              }
            ]
          },
          "permissions": {
            "allow": ["resource.read", "resource.content.read"],
            "network": false,
            "filesystem": false
          }
        }
        "#,
    )
    .unwrap();
    write_empty_wasm_lock(&package, "mindustry");

    let (registry, directory_registry, action_registry) = registries(&root).unwrap();
    let definition = registry
        .get(&ResourceKind::try_new("mindustry:mod").unwrap())
        .unwrap();

    assert_eq!(definition.label(), "Mindustry Mod");
    assert_eq!(definition.origin().to_string(), "plugin:mindustry");
    let directory_definition = directory_registry
        .get(&DirectoryKind::try_new("mindustry:workspace").unwrap())
        .unwrap();
    assert_eq!(directory_definition.label(), "Mindustry Workspace");
    assert_eq!(
        directory_definition.parent(),
        Some(&DirectoryKind::default())
    );
    assert_eq!(
        directory_definition.origin().to_string(),
        "plugin:mindustry"
    );
    assert!(
        actions_for_kind(&registry, &action_registry, definition.kind())
            .iter()
            .any(|action| action.id().as_str() == "mindustry.download")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn directory_registry_includes_host_builtin_default_kind() {
    let (_, registry, _) = registries(&unique_temp_path("no-plugins")).unwrap();
    let default = DirectoryKind::default();
    let definition = registry.get(&default).unwrap();

    assert!(definition.parent().is_none());
    assert_eq!(definition.label(), "Directory");
    assert_eq!(definition.origin().to_string(), "builtin:core.directory");
    assert_eq!(registry.lineage(&default), vec![default]);
}

#[test]
fn registry_loads_format_plugin_as_independent_kind() {
    let root = unique_temp_path("plugin-kind");
    let package = root.join("epub");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("plugin.wasm"), []).unwrap();
    std::fs::write(
        package.join("manifest.json"),
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
            "wasi": false,
            "plugin_api": "asset-hub.plugin-api@2"
          },
          "capabilities": {
            "resource_kinds": [
              {
                "kind": "azvs:epub",
                "parent": "core:resource",
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
                "output": {"views": ["html"]}
              },
              {
                "id": "azvs.epub.thumbnail",
                "provides": "thumbnail",
                "label": "EPUB Thumbnail",
                "handler": "render_epub_thumbnail",
                "applies_to": {
                  "kinds": ["azvs:epub"]
                },
                "access": "read",
                "output": {"views": ["media"]},
                "ui": {
                  "locations": ["resource_list_thumbnail"]
                }
              }
            ]
          },
          "permissions": {
            "allow": ["resource.read", "resource.content.read"],
            "network": false,
            "filesystem": false
          }
        }
        "#,
    )
    .unwrap();
    write_empty_wasm_lock(&package, "epub");

    let (registry, _, action_registry) = registries(&root).unwrap();
    let epub = registry
        .get(&ResourceKind::try_new("azvs:epub").unwrap())
        .unwrap();

    assert_eq!(epub.label(), "EPUB");
    assert_eq!(epub.origin().to_string(), "plugin:epub");
    let actions = actions_for_kind(&registry, &action_registry, epub.kind());
    assert!(
        actions
            .iter()
            .any(|action| action.id().as_str() == "azvs.epub.render")
    );
    assert!(
        actions
            .iter()
            .any(|action| action.id().as_str() == "azvs.epub.thumbnail")
    );
    assert!(
        actions
            .iter()
            .all(|action| action.id().as_str() != "core.resource.thumbnail")
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
    let package = root.join("mp4-tools");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("plugin.wasm"), []).unwrap();
    std::fs::write(
        package.join("manifest.json"),
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
            "wasi": false,
            "plugin_api": "asset-hub.plugin-api@2"
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
                "id": "mp4-tools.inspect",
                "label": "Inspect MP4",
                "handler": "inspect_mp4",
                "applies_to": {
                  "kinds": ["test:mp4"]
                },
                "access": "read",
                "output": {"views": ["json"]}
              }
            ]
          },
          "permissions": {
            "allow": ["resource.read", "resource.content.read"],
            "network": false,
            "filesystem": false
          }
        }
        "#,
    )
    .unwrap();
    write_empty_wasm_lock(&package, "mp4-tools");

    let (registry, _, action_registry) = registries(&root).unwrap();
    let video = registry
        .get(&ResourceKind::try_new("test:mp4").unwrap())
        .unwrap();
    let action = actions_for_kind(&registry, &action_registry, video.kind())
        .into_iter()
        .find(|action| action.id().as_str() == "mp4-tools.inspect")
        .unwrap();

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
    let mut definitions = Vec::new();
    let definition = definition_from_parts(
        ResourceKind::DEFAULT,
        "Resource",
        None,
        true,
        ResourceContentMatcher::default(),
        DefinitionOrigin::builtin_static("test"),
    )
    .unwrap();
    push_definition(&mut definitions, definition.clone()).unwrap();
    let error = push_definition(&mut definitions, definition).unwrap_err();

    assert!(error.to_string().contains("duplicate resource kind"));
}

#[test]
fn registry_rejects_duplicate_global_action_ids() {
    let root = unique_temp_path("duplicate-action");
    let package = root.join("duplicate-download");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("plugin.wasm"), []).unwrap();
    std::fs::write(
        package.join("manifest.json"),
        r#"
        {
          "manifest_version": 2,
          "plugin": {
            "id": "duplicate-download",
            "name": "Duplicate Preview",
            "version": "0.1.0",
            "publisher": "test",
            "description": "Duplicate action id test plugin."
          },
          "runtime": {
            "type": "extism",
            "plugin_api": "asset-hub.plugin-api@2"
          },
          "capabilities": {
            "resource_kinds": [],
            "resource_actions": [
              {
                "id": "core.resource.download",
                "label": "Duplicate Download",
                "handler": "duplicate_download",
                "applies_to": {
                  "kinds": ["core:resource"]
                },
                "access": "read",
                "output": {"views": ["download"]}
              }
            ]
          },
          "permissions": {
            "allow": ["resource.read", "resource.content.read"],
            "network": false,
            "filesystem": false
          }
        }
        "#,
    )
    .unwrap();
    write_empty_wasm_lock(&package, "duplicate-download");

    let error = action_registry(&root).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("duplicate global resource action `core.resource.download`")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn thumbnail_capabilities_require_a_single_nearest_provider() {
    let definitions = vec![ResourceKindDefinition::new(
        ResourceKind::default(),
        "Resource",
        true,
        DefinitionOrigin::builtin_static("test"),
    )];
    let generic = ResourceActionDefinition::new_static("core.resource.thumbnail", "Thumbnail")
        .with_static_provides(Some("thumbnail"))
        .with_kinds(["core:resource"])
        .with_output(ActionOutputContract {
            views: vec!["media".to_string()],
        })
        .with_ui(ActionUi {
            locations: vec!["resource_list_thumbnail".to_string()],
            ..ActionUi::default()
        });
    let competing = ResourceActionDefinition::new_static("example.resource.thumbnail", "Thumbnail")
        .with_static_provides(Some("thumbnail"))
        .with_kinds(["core:resource"])
        .with_output(ActionOutputContract {
            views: vec!["media".to_string()],
        })
        .with_ui(ActionUi {
            locations: vec!["resource_list_thumbnail".to_string()],
            ..ActionUi::default()
        });
    assert!(
        validate_resource_action_capabilities(&definitions, &[generic.clone(), competing])
            .unwrap_err()
            .to_string()
            .contains("has multiple nearest `thumbnail` providers")
    );
    let misplaced = ResourceActionDefinition::new_static("example.preview", "Preview")
        .with_kinds(["core:resource"])
        .with_output(ActionOutputContract {
            views: vec!["media".to_string()],
        })
        .with_ui(ActionUi {
            locations: vec!["resource_list_thumbnail".to_string()],
            ..ActionUi::default()
        });
    assert!(
        validate_resource_action_capabilities(&definitions, &[generic, misplaced])
            .unwrap_err()
            .to_string()
            .contains("must pair `resource_list_thumbnail` with capability `thumbnail`")
    );
    let unsupported = ResourceActionDefinition::new_static("example.unsupported", "Unsupported")
        .with_static_provides(Some("resource.thumbnail"));
    assert!(
        validate_resource_action_capabilities(&definitions, &[unsupported])
            .unwrap_err()
            .to_string()
            .contains("provides unsupported capability `resource.thumbnail`")
    );

    let generic = DirectoryActionDefinition::new_static("core.directory.thumbnail", "Thumbnail")
        .with_static_provides(Some("thumbnail"))
        .with_kinds(["core:directory"])
        .with_output(ActionOutputContract {
            views: vec!["media".to_string()],
        })
        .with_ui(ActionUi {
            locations: vec!["directory_list_thumbnail".to_string()],
            ..ActionUi::default()
        });
    let competing =
        DirectoryActionDefinition::new_static("example.directory.thumbnail", "Thumbnail")
            .with_static_provides(Some("thumbnail"))
            .with_kinds(["core:directory"])
            .with_output(ActionOutputContract {
                views: vec!["media".to_string()],
            })
            .with_ui(ActionUi {
                locations: vec!["directory_list_thumbnail".to_string()],
                ..ActionUi::default()
            });
    struct DirectoryKinds(Vec<DirectoryKindDefinition>);
    impl DirectoryKindRegistry for DirectoryKinds {
        fn definitions(&self) -> &[DirectoryKindDefinition] {
            &self.0
        }
    }
    let kinds = DirectoryKinds(vec![DirectoryKindDefinition::new(
        DirectoryKind::default(),
        "Directory",
        DefinitionOrigin::builtin_static("test"),
    )]);
    assert!(
        validate_directory_action_capabilities(&kinds, &[generic.clone(), competing])
            .unwrap_err()
            .to_string()
            .contains("has multiple nearest `thumbnail` providers")
    );
    let misplaced = DirectoryActionDefinition::new_static("example.preview", "Preview")
        .with_kinds(["core:directory"])
        .with_output(ActionOutputContract {
            views: vec!["media".to_string()],
        })
        .with_ui(ActionUi {
            locations: vec!["directory_list_thumbnail".to_string()],
            ..ActionUi::default()
        });
    assert!(
        validate_directory_action_capabilities(&kinds, &[generic, misplaced])
            .unwrap_err()
            .to_string()
            .contains("must pair `directory_list_thumbnail` with capability `thumbnail`")
    );
    let unsupported = DirectoryActionDefinition::new_static("example.unsupported", "Unsupported")
        .with_static_provides(Some("directory.thumbnail"));
    assert!(
        validate_directory_action_capabilities(&kinds, &[unsupported])
            .unwrap_err()
            .to_string()
            .contains("provides unsupported capability `directory.thumbnail`")
    );
}

fn write_empty_wasm_lock(root: &std::path::Path, plugin_id: &str) {
    std::fs::write(
        root.join("manifest.lock.json"),
        format!(
            r#"{{
              "manifest_version": 2,
              "plugin_id": "{plugin_id}",
              "integrity": {{
                "plugin.wasm": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
              }}
            }}"#
        ),
    )
    .unwrap();
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-kind-{name}-{}", uuid::Uuid::now_v7()))
}
