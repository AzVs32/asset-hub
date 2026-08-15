use super::*;
use crate::domain::{
    ActionOutputContract, ActionUi, DirectoryActionId, DirectoryKind, ResourceActionId,
};

fn directory_service_with_inherited_actions() -> crate::service::DirectoryService {
    let repository = Arc::new(InMemoryResourceRepository::default());
    let storage = Arc::new(InMemoryBlobStorage::default());
    let core_kind = DirectoryKind::default();
    let hello_kind = DirectoryKind::try_new("azvs:directory.hello").unwrap();
    let kinds = Arc::new(InMemoryDirectoryKindRegistry {
        definitions: vec![
            DirectoryKindDefinition::new(
                core_kind.clone(),
                "Directory",
                DefinitionOrigin::builtin_static("core.directory"),
            ),
            DirectoryKindDefinition::new(
                hello_kind,
                "Hello Directory",
                DefinitionOrigin::plugin("azvs.directory.hello").unwrap(),
            )
            .with_parent(Some(core_kind)),
        ],
    });
    let actions = Arc::new(InMemoryDirectoryActionRegistry {
        actions: vec![
            DirectoryActionDefinition::new_static("core.directory.download", "Download")
                .with_kinds([DirectoryKind::DEFAULT]),
            DirectoryActionDefinition::new_static("core.directory.delete", "Delete")
                .with_access(ActionAccess::Write)
                .with_kinds([DirectoryKind::DEFAULT])
                .with_output(ActionOutputContract {
                    views: Vec::new(),
                    effects: vec!["delete".to_string()],
                }),
            DirectoryActionDefinition::new_static("core.directory.thumbnail", "Thumbnail")
                .with_static_provides(Some("thumbnail"))
                .with_kinds([DirectoryKind::DEFAULT])
                .with_output(ActionOutputContract {
                    views: vec!["media".to_string()],
                    effects: Vec::new(),
                })
                .with_ui(ActionUi {
                    locations: vec!["directory_thumbnail".to_string()],
                    ..ActionUi::default()
                }),
            DirectoryActionDefinition::new_static(
                "azvs.directory.hello.workspace",
                "Hello workspace",
            )
            .with_static_provides(Some("workspace"))
            .with_kinds(["azvs:directory.hello"])
            .with_output(ActionOutputContract {
                views: vec!["plugin_frame".to_string()],
                effects: Vec::new(),
            })
            .with_ui(ActionUi {
                locations: vec!["directory_workspace".to_string()],
                ..ActionUi::default()
            }),
        ],
    });

    crate::service::DirectoryService::new(repository.clone(), repository, storage, kinds)
        .with_actions(actions, Arc::new(StaticDirectoryActionExecutor))
}

fn directory_service_with_placement_rules() -> crate::service::DirectoryService {
    let repository = Arc::new(InMemoryResourceRepository::default());
    let storage = Arc::new(InMemoryBlobStorage::default());
    let core = DirectoryKind::default();
    let games = DirectoryKind::try_new("azvs.games:directory").unwrap();
    let item = DirectoryKind::try_new("azvs.games:directory:item").unwrap();
    let special_item = DirectoryKind::try_new("azvs.games:directory:item:special").unwrap();
    let unrelated = DirectoryKind::try_new("azvs:unrelated").unwrap();
    let kinds = Arc::new(InMemoryDirectoryKindRegistry {
        definitions: vec![
            DirectoryKindDefinition::new(
                core.clone(),
                "Directory",
                DefinitionOrigin::builtin_static("core.directory"),
            ),
            DirectoryKindDefinition::new(
                games.clone(),
                "Games",
                DefinitionOrigin::plugin("azvs.games").unwrap(),
            )
            .with_parent(Some(core))
            .with_default_child_kind(Some(item.clone())),
            DirectoryKindDefinition::new(
                item.clone(),
                "Game Item",
                DefinitionOrigin::plugin("azvs.games").unwrap(),
            )
            .with_parent(Some(games.clone()))
            .with_allowed_parent_kinds([games]),
            DirectoryKindDefinition::new(
                special_item,
                "Special Game Item",
                DefinitionOrigin::plugin("azvs.games").unwrap(),
            )
            .with_parent(Some(item)),
            DirectoryKindDefinition::new(
                unrelated,
                "Unrelated",
                DefinitionOrigin::plugin("azvs.unrelated").unwrap(),
            )
            .with_parent(Some(DirectoryKind::default())),
        ],
    });
    crate::service::DirectoryService::new(repository.clone(), repository, storage, kinds)
}

#[test]
fn directory_kind_placement_is_enforced_for_create_move_and_parent_kind_changes() {
    let service = directory_service_with_placement_rules();
    let root = block_on(service.root()).unwrap();
    let games_kind = DirectoryKind::try_new("azvs.games:directory").unwrap();
    let item_kind = DirectoryKind::try_new("azvs.games:directory:item").unwrap();
    let special_item_kind = DirectoryKind::try_new("azvs.games:directory:item:special").unwrap();
    let games = block_on(service.create_with_kind(&root, "games", games_kind)).unwrap();

    let automatic_item = block_on(service.create(games.location(), "automatic-item")).unwrap();
    assert_eq!(
        block_on(service.find_by_id(&automatic_item.id()))
            .unwrap()
            .directory()
            .kind(),
        &item_kind
    );

    assert!(matches!(
        block_on(service.create_with_kind(&root, "outside", item_kind.clone())),
        Err(CoreError::Conflict { .. })
    ));
    assert!(matches!(
        block_on(service.create_with_kind(&root, "special-outside", special_item_kind.clone())),
        Err(CoreError::Conflict { .. })
    ));

    let item = block_on(service.create_with_kind(games.location(), "inside", item_kind)).unwrap();
    block_on(service.create_with_kind(games.location(), "special-inside", special_item_kind))
        .unwrap();
    assert!(matches!(
        block_on(service.move_to(&item.id(), &DirectoryId::root())),
        Err(CoreError::Conflict { .. })
    ));
    assert!(matches!(
        block_on(service.change_kind(&games.id(), DirectoryKind::default())),
        Err(CoreError::Conflict { .. })
    ));
}

#[test]
fn changing_a_directory_to_games_reclassifies_only_generic_direct_children() {
    let service = directory_service_with_placement_rules();
    let root = block_on(service.root()).unwrap();
    let library = block_on(service.create(&root, "library")).unwrap();
    let generic = block_on(service.create(&library, "generic-game")).unwrap();
    let unrelated = block_on(service.create_with_kind(
        &library,
        "preserved",
        DirectoryKind::try_new("azvs:unrelated").unwrap(),
    ))
    .unwrap();

    block_on(service.change_kind(
        &library.id(),
        DirectoryKind::try_new("azvs.games:directory").unwrap(),
    ))
    .unwrap();

    assert_eq!(
        block_on(service.find_by_id(&generic.id()))
            .unwrap()
            .directory()
            .kind()
            .as_str(),
        "azvs.games:directory:item"
    );
    assert_eq!(
        block_on(service.find_by_id(&unrelated.id()))
            .unwrap()
            .directory()
            .kind()
            .as_str(),
        "azvs:unrelated"
    );
}

#[test]
fn directory_subkind_inherits_ancestor_actions_and_keeps_its_workspace() {
    let service = directory_service_with_inherited_actions();
    let directory = Directory::new_with_kind(
        DirectoryId::root(),
        "directory_demo",
        DirectoryKind::try_new("azvs:directory.hello").unwrap(),
    )
    .unwrap();

    let actions = service.describe_actions(&directory).unwrap();
    let ids = actions
        .available_actions()
        .iter()
        .map(|action| action.id().as_str())
        .collect::<HashSet<_>>();

    assert_eq!(
        ids,
        HashSet::from([
            "azvs.directory.hello.workspace",
            "core.directory.download",
            "core.directory.delete",
            "core.directory.thumbnail",
        ])
    );
    let thumbnail = service
        .resolve_action(
            &directory,
            &DirectoryActionId::from_static("core.directory.thumbnail"),
        )
        .unwrap();
    assert_eq!(thumbnail.kinds(), [DirectoryKind::DEFAULT]);
}

#[test]
fn root_directory_delete_is_neither_described_nor_resolvable() {
    let service = directory_service_with_inherited_actions();
    let root = Directory::root();
    let delete = DirectoryActionId::from_static("core.directory.delete");

    assert!(
        service
            .describe_actions(&root)
            .unwrap()
            .available_actions()
            .iter()
            .all(|action| action.id().as_str() != delete.as_str())
    );
    assert!(matches!(
        service.resolve_action(&root, &delete),
        Err(CoreError::Unsupported {
            subject: "directory action",
            ..
        })
    ));
}

#[test]
fn action_content_delivery_never_loads_unrequested_content() {
    use crate::domain::{ResourceActionContentDelivery, ResourceActionRequirements};
    let policy = test_resource_action_policy();

    let without_content = ResourceActionDefinition::new_static("test.inspect", "Inspect");
    assert_eq!(
        resolved_content_delivery(&without_content, 1, &policy),
        None
    );

    let required = |delivery| {
        ResourceActionDefinition::new_static("test.inspect", "Inspect").with_requirements(
            ResourceActionRequirements {
                content: true,
                content_delivery: delivery,
            },
        )
    };
    assert_eq!(
        resolved_content_delivery(
            &required(ResourceActionContentDelivery::Inline),
            policy.max_inline_content_bytes() + 1,
            &policy,
        ),
        Some(ResourceActionContentDelivery::Inline)
    );
    assert_eq!(
        resolved_content_delivery(
            &required(ResourceActionContentDelivery::Reference),
            1,
            &policy,
        ),
        Some(ResourceActionContentDelivery::Reference)
    );
    assert_eq!(
        resolved_content_delivery(
            &required(ResourceActionContentDelivery::Auto),
            policy.max_inline_content_bytes(),
            &policy,
        ),
        Some(ResourceActionContentDelivery::Inline)
    );
    assert_eq!(
        resolved_content_delivery(
            &required(ResourceActionContentDelivery::Auto),
            policy.max_inline_content_bytes() + 1,
            &policy,
        ),
        Some(ResourceActionContentDelivery::Reference)
    );
}

#[test]
fn resource_without_content_describes_only_actions_without_content_requirements() {
    let (service, _, _) = service();
    let resource = command::build_resource(
        "contentless".to_string(),
        DirectoryId::root(),
        Some(ResourceKind::try_new("example:metadata").unwrap()),
    )
    .build()
    .unwrap();

    let actions = service
        .actions()
        .describe_resource_actions(&resource)
        .unwrap();
    let ids = actions
        .available_actions()
        .iter()
        .map(|action| action.id().as_str())
        .collect::<Vec<_>>();

    assert_eq!(ids, vec!["resource.inspect", "core.resource.thumbnail"]);
}

#[test]
fn resource_without_content_rejects_direct_content_action_execution() {
    let (service, repository, _) = service();
    let resource = command::build_resource(
        "contentless".to_string(),
        DirectoryId::root(),
        Some(ResourceKind::try_new("example:metadata").unwrap()),
    )
    .build()
    .unwrap();
    block_on(repository.save(&resource)).unwrap();

    let error = block_on(service.actions().execute_resource_action(
        &resource.id(),
        ExecuteResourceAction::new(ResourceActionId::from_static("test.content.extract"), None),
    ))
    .unwrap_err();

    assert!(matches!(
        error,
        CoreError::Unsupported {
            subject: "resource action",
            value
        } if value == "test.content.extract"
    ));
}

#[test]
fn execute_action_rejects_a_stale_caller_snapshot() {
    let (service, _, blob_storage) = service();
    let key = StorageKey::new("docs/stale-note.md").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "stale-note.md",
                key.clone(),
                Bytes::from_static(b"# Current"),
            )
            .with_kind(ResourceKind::try_new("example:document").unwrap())
            .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let stale = resource.revision() + 1;

    let error = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new(
                ResourceActionId::from_static("example.content.edit"),
                Some(stale),
            )
            .with_input(json!({"markdown": "# Stale"})),
        ),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::RevisionConflict { .. }));
    assert_eq!(
        blob_storage.get_sync(&key).unwrap(),
        Bytes::from_static(b"# Current")
    );
}

#[test]
fn write_action_cleanup_failure_keeps_a_recoverable_intent() {
    let (service, _repository, blob_storage) = service();
    let key = StorageKey::new("docs/note.md").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("note.md", key, Bytes::from_static(b"# Old"))
                .with_kind(ResourceKind::try_new("example:document").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    blob_storage.fail_next_delete();

    let error = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new(
                ResourceActionId::from_static("example.content.edit"),
                Some(resource.revision()),
            )
            .with_input(json!({"markdown": "# New"})),
        ),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::Storage { .. }));
    assert!(blob_storage.contains_fragment(".asset-hub/content-backups/"));
    assert_eq!(
        block_on(service.content_replacements.list_pending())
            .unwrap()
            .len(),
        1
    );

    assert_eq!(block_on(service.resume_content_replacements()).unwrap(), 1);
    assert!(!blob_storage.contains_fragment(".asset-hub/content-backups/"));
    assert!(!blob_storage.contains_fragment(".asset-hub/uploads/replacement-"));
    assert!(
        block_on(service.content_replacements.list_pending())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn describe_resource_actions_uses_declared_content_matchers() {
    let (service, _, _) = service();
    let pdf = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "book",
                StorageKey::new("books/book.pdf").unwrap(),
                Bytes::from_static(b"%PDF-1.4"),
            )
            .with_kind(ResourceKind::try_new("example:document").unwrap())
            .with_mime_type("application/pdf"),
        ),
    )
    .unwrap();
    let text = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "book",
                StorageKey::new("books/book.txt").unwrap(),
                Bytes::from_static(b"hello"),
            )
            .with_kind(ResourceKind::try_new("example:document").unwrap())
            .with_mime_type("text/plain"),
        ),
    )
    .unwrap();

    let pdf_actions = service.actions().describe_resource_actions(&pdf).unwrap();
    let text_actions = service.actions().describe_resource_actions(&text).unwrap();
    let has_action = |actions: &ResourceActions, id: &str| {
        actions
            .available_actions()
            .iter()
            .any(|action| action.id().as_str() == id)
    };

    assert!(has_action(&pdf_actions, "test.content.extract"));
    assert!(has_action(&pdf_actions, "test.content.thumbnail"));
    assert!(!has_action(&pdf_actions, "core.resource.thumbnail"));
    assert!(!has_action(&pdf_actions, "example.content.read"));
    assert!(has_action(&text_actions, "test.content.extract"));
    assert!(!has_action(&text_actions, "test.content.thumbnail"));
    assert!(has_action(&text_actions, "core.resource.thumbnail"));
    assert!(has_action(&text_actions, "example.content.read"));
}
