use super::*;

struct Registry(Vec<ResourceActionDefinition>);

impl ResourceActionRegistry for Registry {
    fn actions(&self) -> &[ResourceActionDefinition] {
        &self.0
    }
}

#[test]
fn actions_are_selected_from_the_supplied_kind_lineage() {
    let registry = Registry(vec![
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
