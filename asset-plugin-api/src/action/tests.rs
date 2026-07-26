use super::*;

#[test]
fn action_requirements_are_the_only_content_requirement_state() {
    let action = ResourceActionDefinition::new("example.document.inspect", "Inspect")
        .with_requirements(ResourceActionRequirements {
            content: true,
            content_delivery: ResourceActionContentDelivery::Reference,
        });

    let value = serde_json::to_value(action).unwrap();
    assert_eq!(value["requires"]["content"], true);
    assert_eq!(value["requires"]["content_delivery"], "reference");
    assert!(value["requires"].get("resource").is_none());
    assert!(value.get("requires_content").is_none());
    assert!(value.get("content_delivery").is_none());
}

#[test]
fn action_applies_to_matches_kind_mime_and_extension() {
    let applies_to = ResourceActionAppliesTo::new()
        .with_kinds(["core:video"])
        .with_mime_types(["video/*"])
        .with_extensions(["mp4"]);

    assert!(applies_to.matches_resource("core:video", Some("video/mp4"), Some("demo.bin")));
    assert!(applies_to.matches_resource("CORE:VIDEO", None, Some("demo.mp4")));
    assert!(!applies_to.matches_resource("core:image", Some("video/mp4"), Some("demo.mp4")));
    assert!(!applies_to.matches_resource("core:video", Some("application/pdf"), Some("demo.pdf")));
}

#[test]
fn matcher_deserialization_preserves_normalized_invariants() {
    let matcher: ResourceContentMatcher = serde_json::from_value(serde_json::json!({
        "mime_types": [" Text/Markdown "],
        "extensions": ["MD"]
    }))
    .unwrap();

    assert_eq!(matcher.mime_types(), ["text/markdown"]);
    assert_eq!(matcher.extensions(), [".md"]);
    assert!(matcher.matches_content(Some("TEXT/MARKDOWN"), None));
    assert!(matcher.matches_content(None, Some("README.MD")));
    assert!(
        serde_json::from_value::<ResourceContentMatcher>(serde_json::json!({
            "mime_types": ["  "]
        }))
        .is_err()
    );
}
