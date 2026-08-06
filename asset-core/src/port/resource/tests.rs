use super::*;
use crate::domain::{ResourceActionDefinition, ResourceContentMatcher, ResourceKind};

struct ActionRegistry(Vec<ResourceActionDefinition>);

impl ResourceActionRegistry for ActionRegistry {
    fn actions(&self) -> &[ResourceActionDefinition] {
        &self.0
    }
}

#[test]
fn actions_are_selected_from_the_supplied_kind_lineage() {
    let registry = ActionRegistry(vec![
        ResourceActionDefinition::new_static("text.open", "Open").with_kinds(["core:text"]),
        ResourceActionDefinition::new_static("image.view", "View").with_kinds(["core:image"]),
    ]);
    let lineage = vec![
        ResourceKind::try_new("code:c").unwrap(),
        ResourceKind::try_new("core:text").unwrap(),
    ];

    let actions = registry.actions_for_kinds(&lineage);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id().as_str(), "text.open");
}

#[test]
fn a_specific_provider_replaces_the_selected_generic_action() {
    let registry = ActionRegistry(vec![
        ResourceActionDefinition::new_static("core.resource.thumbnail", "Thumbnail")
            .with_static_provides(Some("thumbnail"))
            .with_kinds(["core:resource"]),
        ResourceActionDefinition::new_static("azvs.epub.thumbnail", "EPUB Thumbnail")
            .with_static_provides(Some("thumbnail"))
            .with_kinds(["azvs:epub"]),
    ]);
    let lineage = vec![
        ResourceKind::try_new("azvs:epub").unwrap(),
        ResourceKind::try_new("core:resource").unwrap(),
    ];

    let actions = registry.actions_for_kinds(&lineage);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id().as_str(), "azvs.epub.thumbnail");
}

#[test]
fn capability_candidates_keep_fallback_providers_until_resource_filtering() {
    let registry = ActionRegistry(vec![
        ResourceActionDefinition::new_static("core.resource.thumbnail", "Thumbnail")
            .with_static_provides(Some("thumbnail"))
            .with_kinds(["core:resource"]),
        ResourceActionDefinition::new_static("azvs.epub.thumbnail", "EPUB Thumbnail")
            .with_static_provides(Some("thumbnail"))
            .with_kinds(["azvs:epub"]),
    ]);
    let lineage = vec![
        ResourceKind::try_new("azvs:epub").unwrap(),
        ResourceKind::try_new("core:resource").unwrap(),
    ];

    let candidates = registry.action_candidates_for_kinds(&lineage);

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].id().as_str(), "azvs.epub.thumbnail");
    assert_eq!(candidates[1].id().as_str(), "core.resource.thumbnail");
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
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:image").unwrap(),
                "Image",
                true,
            )
            .with_detect(ResourceContentMatcher::new().with_extensions([".png"])),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:resource").unwrap(),
                "File",
                true,
            ),
        ],
    };

    assert_eq!(
        registry.detect_content_kind(None, Some("images/demo.png")),
        Some(ResourceKind::try_new("core:image").unwrap())
    );
}

#[test]
fn supports_arbitrary_depth_lineage_inheritance_and_leaf_detection() {
    let text = ResourceKind::try_new("core:text").unwrap();
    let code = ResourceKind::try_new("core:code").unwrap();
    let c = ResourceKind::try_new("code:c").unwrap();
    let registry = KindRegistry {
        definitions: vec![
            ResourceKindDefinition::new(text.clone(), "Text", true),
            ResourceKindDefinition::new(code.clone(), "Code", true).with_parent(Some(text.clone())),
            ResourceKindDefinition::new(c.clone(), "C", true)
                .with_parent(Some(code.clone()))
                .with_detect(ResourceContentMatcher::new().with_extensions([".c", ".h"])),
        ],
    };

    assert_eq!(registry.lineage(&c), vec![c.clone(), code.clone(), text]);
    assert!(registry.descendants(&code).contains(&c));
    assert_eq!(
        registry.detect_content_kind(Some("text/plain"), Some("src/main.c")),
        Some(c)
    );
}
