use super::*;

#[derive(Default)]
struct TestRegistry {
    definitions: Vec<ResourceKindDefinition>,
}

impl ResourceKindRegistry for TestRegistry {
    fn definitions(&self) -> &[ResourceKindDefinition] {
        &self.definitions
    }
}

#[test]
fn kind_detection_uses_only_kind_matchers() {
    let registry = TestRegistry {
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
    let registry = TestRegistry {
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
