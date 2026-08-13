//! Action 领域模型的匹配与归一化行为测试。

use super::*;

#[test]
fn action_applies_to_matches_kind_mime_and_extension() {
    let applies_to = ResourceActionAppliesTo::new()
        .with_kinds(["core:video"])
        .with_mime_types(["video/*"])
        .with_extensions(["mp4"]);

    assert!(applies_to.matches_resource("core:video", Some("video/mp4"), Some("demo.bin")));
    assert!(applies_to.matches_resource("core:video", None, Some("demo.mp4")));
    assert!(!applies_to.matches_resource("CORE:VIDEO", None, Some("demo.mp4")));
    assert!(!applies_to.matches_resource("core:image", Some("video/mp4"), Some("demo.mp4")));
    assert!(!applies_to.matches_resource("core:video", Some("application/pdf"), Some("demo.pdf")));
}

#[test]
fn matcher_construction_preserves_normalized_invariants() {
    let matcher = ResourceContentMatcher::new()
        .with_mime_types([" Text/Markdown "])
        .with_extensions(["MD"]);

    assert_eq!(matcher.mime_types(), ["text/markdown"]);
    assert_eq!(matcher.extensions(), [".md"]);
    assert!(matcher.matches_content(Some("TEXT/MARKDOWN"), None));
    assert!(matcher.matches_content(None, Some("README.MD")));
}

#[test]
fn directory_actions_share_the_common_shell_but_keep_directory_contracts() {
    let action = DirectoryActionDefinition::new_static("example.collection.organize", "Organize")
        .with_access(ActionAccess::Write)
        .with_kinds(["example:collection"])
        .with_requirements(DirectoryActionRequirements {
            children: true,
            resources: true,
        });

    assert!(action.matches_exact_kind("example:collection"));
    assert!(!action.matches_exact_kind("EXAMPLE:COLLECTION"));
    assert!(!action.matches_exact_kind("core:directory"));
    assert_eq!(action.access(), ActionAccess::Write);
    assert!(action.requirements().children);
}

#[test]
fn action_ids_reject_blank_non_canonical_and_invalid_values() {
    assert!(matches!(
        ActionId::new(""),
        Err(ActionIdError::Blank { .. })
    ));
    assert!(matches!(
        ActionId::new(" example.inspect"),
        Err(ActionIdError::NonCanonical { .. })
    ));
    assert!(matches!(
        ActionId::new("Example.inspect"),
        Err(ActionIdError::InvalidFormat { .. })
    ));
    assert!(ActionId::new("example:resource.inspect").is_err());
    assert_eq!(
        ActionId::new("example.resource.inspect").unwrap().as_str(),
        "example.resource.inspect"
    );
}

#[test]
fn capability_ids_use_the_narrower_capability_format() {
    assert!(ActionCapabilityId::new("thumbnail").is_ok());
    assert!(ActionCapabilityId::new("text_read.v2").is_ok());
    assert!(matches!(
        ActionCapabilityId::new("resource:thumbnail"),
        Err(ActionIdError::InvalidFormat { .. })
    ));
}

#[test]
fn effect_only_actions_are_declared_without_a_view() {
    let action = ResourceActionDefinition::new_static("core.resource.delete", "Delete")
        .with_access(ActionAccess::Write)
        .with_output(ActionOutputContract {
            views: Vec::new(),
            effects: vec!["delete".to_string()],
        })
        .with_ui(ActionUi {
            destructive: true,
            confirmation: Some("Delete {name}?".to_string()),
            ..ActionUi::default()
        });

    assert!(action.output().views.is_empty());
    assert_eq!(action.output().effects, ["delete"]);
    assert!(action.ui().destructive);
}
