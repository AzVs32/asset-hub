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
use asset_core::port::DirectoryKindRegistry;
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

fn action_registry(packages_root: &Path) -> Result<DefaultResourceActionRegistry, CoreError> {
    registries(packages_root).map(|(_, _, actions)| actions)
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
fn directory_registry_rejects_unknown_allowed_parent_kinds() {
    let definitions = vec![
        DirectoryKindDefinition::new(
            DirectoryKind::try_new("core:directory").unwrap(),
            "Directory",
            DefinitionOrigin::builtin_static("core.directory"),
        ),
        DirectoryKindDefinition::new(
            DirectoryKind::try_new("plugin:directory:games:item").unwrap(),
            "Game",
            DefinitionOrigin::plugin_static("plugin.games"),
        )
        .with_parent(Some(DirectoryKind::try_new("core:directory").unwrap()))
        .with_allowed_parent_kinds([DirectoryKind::try_new("plugin:directory:missing").unwrap()]),
    ];

    assert!(
        super::directory_registry::validate_hierarchy(&definitions)
            .unwrap_err()
            .to_string()
            .contains("allows unknown parent kind")
    );
}

#[test]
fn directory_registry_rejects_an_unknown_default_child_kind() {
    let definitions = vec![
        DirectoryKindDefinition::new(
            DirectoryKind::try_new("core:directory").unwrap(),
            "Directory",
            DefinitionOrigin::builtin_static("core.directory"),
        )
        .with_default_child_kind(Some(
            DirectoryKind::try_new("plugin:directory:missing").unwrap(),
        )),
    ];

    assert!(
        super::directory_registry::validate_hierarchy(&definitions)
            .unwrap_err()
            .to_string()
            .contains("unknown default child kind")
    );
}

#[test]
fn directory_registry_requires_default_children_to_inherit_from_the_parent_kind() {
    let core = DirectoryKind::try_new("core:directory").unwrap();
    let games = DirectoryKind::try_new("plugin:directory:games").unwrap();
    let unrelated = DirectoryKind::try_new("plugin:directory:unrelated").unwrap();
    let definitions = vec![
        DirectoryKindDefinition::new(
            core.clone(),
            "Directory",
            DefinitionOrigin::builtin_static("core.directory"),
        ),
        DirectoryKindDefinition::new(
            games.clone(),
            "Games",
            DefinitionOrigin::plugin_static("plugin.games"),
        )
        .with_parent(Some(core.clone()))
        .with_default_child_kind(Some(unrelated.clone())),
        DirectoryKindDefinition::new(
            unrelated,
            "Unrelated",
            DefinitionOrigin::plugin_static("plugin.unrelated"),
        )
        .with_parent(Some(core)),
    ];

    assert!(
        super::directory_registry::validate_hierarchy(&definitions)
            .unwrap_err()
            .to_string()
            .contains("must inherit")
    );
}

#[test]
fn inherited_action_label_requires_an_ancestor_capability_provider() {
    let definitions = vec![
        definition_from_parts(
            "core:resource",
            "Resource",
            None,
            true,
            ResourceContentMatcher::default(),
            DefinitionOrigin::builtin_static("core.resource"),
        )
        .unwrap(),
        definition_from_parts(
            "core:image",
            "Image",
            Some("core:resource"),
            true,
            ResourceContentMatcher::default(),
            DefinitionOrigin::builtin_static("core.image"),
        )
        .unwrap(),
    ];
    let mut actions = vec![
        ResourceActionDefinition::new_static("example.image.read", "example.image.read")
            .with_static_provides(Some("thumbnail"))
            .with_kinds(["core:image"]),
    ];

    assert!(
        resolve_inherited_resource_action_labels(&definitions, &mut actions, &[0])
            .unwrap_err()
            .to_string()
            .contains("no ancestor provider exists")
    );
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
          "manifest_version": 3,
          "plugin": {
            "id": "duplicate-download",
            "name": "Duplicate Preview",
            "version": "0.1.0",
            "publisher": "test",
            "description": "Duplicate action id test plugin."
          },
          "runtime": {
            "type": "extism",
            "plugin_api": "asset-hub.plugin-api@5"
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
            effects: Vec::new(),
        })
        .with_ui(ActionUi {
            locations: vec!["resource_thumbnail".to_string()],
            ..ActionUi::default()
        });
    let competing = ResourceActionDefinition::new_static("example.resource.thumbnail", "Thumbnail")
        .with_static_provides(Some("thumbnail"))
        .with_kinds(["core:resource"])
        .with_output(ActionOutputContract {
            views: vec!["media".to_string()],
            effects: Vec::new(),
        })
        .with_ui(ActionUi {
            locations: vec!["resource_thumbnail".to_string()],
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
            effects: Vec::new(),
        })
        .with_ui(ActionUi {
            locations: vec!["resource_thumbnail".to_string()],
            ..ActionUi::default()
        });
    assert!(
        validate_resource_action_capabilities(&definitions, &[generic, misplaced])
            .unwrap_err()
            .to_string()
            .contains("must pair `resource_thumbnail` with capability `thumbnail`")
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
            effects: Vec::new(),
        })
        .with_ui(ActionUi {
            locations: vec!["directory_thumbnail".to_string()],
            ..ActionUi::default()
        });
    let competing =
        DirectoryActionDefinition::new_static("example.directory.thumbnail", "Thumbnail")
            .with_static_provides(Some("thumbnail"))
            .with_kinds(["core:directory"])
            .with_output(ActionOutputContract {
                views: vec!["media".to_string()],
                effects: Vec::new(),
            })
            .with_ui(ActionUi {
                locations: vec!["directory_thumbnail".to_string()],
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
            effects: Vec::new(),
        })
        .with_ui(ActionUi {
            locations: vec!["directory_thumbnail".to_string()],
            ..ActionUi::default()
        });
    assert!(
        validate_directory_action_capabilities(&kinds, &[generic, misplaced])
            .unwrap_err()
            .to_string()
            .contains("must pair `directory_thumbnail` with capability `thumbnail`")
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

#[test]
fn directory_workspace_capability_is_exclusive_and_frame_only() {
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
    let workspace = DirectoryActionDefinition::new_static("example.workspace", "Workspace")
        .with_static_provides(Some("workspace"))
        .with_kinds(["core:directory"])
        .with_output(ActionOutputContract {
            views: vec!["plugin_frame".to_string(), "json".to_string()],
            effects: Vec::new(),
        })
        .with_ui(ActionUi {
            locations: vec!["directory_workspace".to_string()],
            ..ActionUi::default()
        });
    validate_directory_action_capabilities(&kinds, std::slice::from_ref(&workspace)).unwrap();

    let competing = DirectoryActionDefinition::new_static("other.workspace", "Workspace")
        .with_static_provides(Some("workspace"))
        .with_kinds(["core:directory"])
        .with_output(ActionOutputContract {
            views: vec!["plugin_frame".to_string()],
            effects: Vec::new(),
        })
        .with_ui(ActionUi {
            locations: vec!["directory_workspace".to_string()],
            ..ActionUi::default()
        });
    assert!(
        validate_directory_action_capabilities(&kinds, &[workspace.clone(), competing])
            .unwrap_err()
            .to_string()
            .contains("multiple nearest `workspace` providers")
    );

    let invalid = workspace.clone().with_output(ActionOutputContract {
        views: vec!["json".to_string()],
        effects: Vec::new(),
    });
    assert!(
        validate_directory_action_capabilities(&kinds, &[invalid])
            .unwrap_err()
            .to_string()
            .contains("must be read-only, support `plugin_frame`, declare no effects")
    );

    let mixed_locations = workspace.with_ui(ActionUi {
        locations: vec![
            "directory_workspace".to_string(),
            "directory_context_menu".to_string(),
        ],
        ..ActionUi::default()
    });
    assert!(
        validate_directory_action_capabilities(&kinds, &[mixed_locations])
            .unwrap_err()
            .to_string()
            .contains("use only `directory_workspace`")
    );
}

fn write_empty_wasm_lock(root: &std::path::Path, plugin_id: &str) {
    std::fs::write(
        root.join("manifest.lock.json"),
        format!(
            r#"{{
              "manifest_version": 3,
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
