//! Action 领域模型的匹配与归一化行为测试。

use super::*;

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
    let action = DirectoryActionDefinition::new("example.collection.organize", "Organize")
        .with_access(ActionAccess::ReadWrite)
        .with_kinds(["example:collection"])
        .with_requirements(DirectoryActionRequirements {
            children: true,
            resources: true,
        });

    assert!(action.matches_directory("EXAMPLE:COLLECTION"));
    assert!(!action.matches_directory("core:directory"));
    assert_eq!(action.access(), ActionAccess::ReadWrite);
    assert!(action.requirements().children);
}
