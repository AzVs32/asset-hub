use super::*;
use crate::domain::ResourceKind;
use asset_plugin_api::{ResourceActionDefinition, ResourceContentMatcher};

struct ActionRegistry(Vec<ResourceActionDefinition>);

impl ResourceActionRegistry for ActionRegistry {
    fn actions(&self) -> &[ResourceActionDefinition] {
        &self.0
    }
}

#[test]
fn actions_are_selected_from_the_supplied_kind_lineage() {
    let registry = ActionRegistry(vec![
        ResourceActionDefinition::new("document.open", "Open").with_kinds(["core:document"]),
        ResourceActionDefinition::new("image.view", "View").with_kinds(["core:image"]),
    ]);
    let lineage = vec![
        ResourceKind::from("code:c"),
        ResourceKind::from("core:document"),
    ];

    let actions = registry.actions_for_kinds(&lineage);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id().as_str(), "document.open");
}

#[derive(Default)]
struct KindRegistry {
    definitions: Vec<ResourceKindDefinition>,
}

impl ResourceKindRegistry for KindRegistry {
    fn definitions(&self) -> &[ResourceKindDefinition] {
        &self.definitions
    }
}

#[test]
fn kind_detection_uses_only_kind_matchers() {
    let registry = KindRegistry {
        definitions: vec![
            ResourceKindDefinition::new(ResourceKind::from("core:image"), "Image", true)
                .with_detect(ResourceContentMatcher::new().with_extensions([".png"])),
            ResourceKindDefinition::new(ResourceKind::from("core:file"), "File", true),
        ],
    };

    assert_eq!(
        registry.detect_content_kind(None, Some("images/demo.png")),
        Some(ResourceKind::from("core:image"))
    );
}

#[test]
fn supports_arbitrary_depth_lineage_inheritance_and_leaf_detection() {
    let document = ResourceKind::from("core:document");
    let code = ResourceKind::from("core:code");
    let c = ResourceKind::from("code:c");
    let registry = KindRegistry {
        definitions: vec![
            ResourceKindDefinition::new(document.clone(), "Document", true),
            ResourceKindDefinition::new(code.clone(), "Code", true)
                .with_parent(Some(document.clone())),
            ResourceKindDefinition::new(c.clone(), "C", true)
                .with_parent(Some(code.clone()))
                .with_detect(ResourceContentMatcher::new().with_extensions([".c", ".h"])),
        ],
    };

    assert_eq!(
        registry.lineage(&c),
        vec![c.clone(), code.clone(), document]
    );
    assert!(registry.descendants(&code).contains(&c));
    assert_eq!(
        registry.detect_content_kind(Some("text/plain"), Some("src/main.c")),
        Some(c)
    );
}
