use super::*;
use crate::domain::{
    DefinitionOrigin, ResourceContentMatcher, ResourceKind, ResourceKindDefinition,
};

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
                ResourceKind::try_new("example:image").unwrap(),
                "Image",
                true,
                DefinitionOrigin::builtin_static("test"),
            )
            .with_detect(ResourceContentMatcher::new().with_extensions([".png"])),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:resource").unwrap(),
                "File",
                true,
                DefinitionOrigin::builtin_static("test"),
            ),
        ],
    };

    assert_eq!(
        registry
            .detect_content_kind(None, Some("images/demo.png"))
            .unwrap(),
        Some(ResourceKind::try_new("example:image").unwrap())
    );
}

#[test]
fn kind_detection_rejects_equally_specific_matches() {
    let registry = KindRegistry {
        definitions: vec![
            ResourceKindDefinition::new(
                ResourceKind::try_new("example:first").unwrap(),
                "First",
                true,
                DefinitionOrigin::builtin_static("test.first"),
            )
            .with_detect(ResourceContentMatcher::new().with_extensions([".demo"])),
            ResourceKindDefinition::new(
                ResourceKind::try_new("example:second").unwrap(),
                "Second",
                true,
                DefinitionOrigin::builtin_static("test.second"),
            )
            .with_detect(ResourceContentMatcher::new().with_extensions([".demo"])),
        ],
    };

    let error = registry
        .detect_content_kind(None, Some("asset.demo"))
        .unwrap_err();

    assert!(error.to_string().contains("example:first, example:second"));
}
