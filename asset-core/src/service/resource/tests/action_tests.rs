use super::*;
use crate::domain::ResourceActionId;

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
        Some(ResourceKind::try_new("doc:markdown").unwrap()),
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
        Some(ResourceKind::try_new("doc:markdown").unwrap()),
    )
    .build()
    .unwrap();
    block_on(repository.save(&resource)).unwrap();

    let error = block_on(service.actions().execute_resource_action(
        &resource.id(),
        ExecuteResourceAction::new(ResourceActionId::from_static("test.text.extract"), None),
    ))
    .unwrap_err();

    assert!(matches!(
        error,
        CoreError::Unsupported {
            subject: "resource action",
            value
        } if value == "test.text.extract"
    ));
}

#[test]
fn execute_content_action_returns_text_for_matching_kind() {
    let (service, _, _) = service();
    let key = StorageKey::new("books/book.txt").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("book", key, Bytes::from_static(b"Hello book"))
                .with_kind(ResourceKind::try_new("core:text").unwrap()),
        ),
    )
    .unwrap();

    let output = block_on(service.actions().execute_resource_action(
        &resource.id(),
        ExecuteResourceAction::new(ResourceActionId::from_static("test.text.extract"), None),
    ))
    .unwrap()
    .unwrap();

    assert_eq!(
        &output.output().view,
        &PluginView::Text(TextView {
            text: "Hello book".to_string()
        })
    );
}

#[test]
fn execute_write_action_replaces_resource_content() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/note.md").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("note.md", key.clone(), Bytes::from_static(b"# Old"))
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();

    let error = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new(ResourceActionId::from_static("azvs.markdown.edit"), None)
                .with_input(json!({"markdown": "# Missing revision"})),
        ),
    )
    .unwrap_err();
    assert!(matches!(error, CoreError::InvalidOperation { .. }));

    let output = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new(
                ResourceActionId::from_static("azvs.markdown.edit"),
                Some(resource.revision()),
            )
            .with_input(json!({"markdown": "# New\n\nUpdated."})),
        ),
    )
    .unwrap()
    .unwrap();

    assert_eq!(output.action().as_str(), "azvs.markdown.edit");
    let updated = repository.find_sync(&resource.id()).unwrap();
    let content = updated.content().unwrap();
    assert!(blob_storage.contains(&key));
    assert_eq!(block_on(service.storage_key(&updated)).unwrap(), key);
    assert!(!blob_storage.contains_fragment(".asset-hub/"));
    assert!(!blob_storage.contains_fragment(".action-replacements/"));
    assert!(!blob_storage.contains_fragment(".action-backups/"));
    assert_eq!(
        blob_storage
            .get_sync(&block_on(service.storage_key(&updated)).unwrap())
            .unwrap(),
        Bytes::from_static(b"# New\n\nUpdated.")
    );
    assert_eq!(content.size(), 15);
    assert_eq!(content.mime_type(), Some("text/markdown"));
    assert_eq!(content.checksum().unwrap().kind(), ChecksumKind::Sha256);
    assert_eq!(
        content.checksum().unwrap().value(),
        hex_sha256(b"# New\n\nUpdated.")
    );
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
            .with_kind(ResourceKind::try_new("core:text").unwrap())
            .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let stale = resource.revision() + 1;

    let error = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new(
                ResourceActionId::from_static("azvs.markdown.edit"),
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
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    blob_storage.fail_next_delete();

    let error = block_on(
        service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new(
                ResourceActionId::from_static("azvs.markdown.edit"),
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
            .with_kind(ResourceKind::try_new("core:text").unwrap())
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
            .with_kind(ResourceKind::try_new("core:text").unwrap())
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

    assert!(has_action(&pdf_actions, "test.text.extract"));
    assert!(has_action(&pdf_actions, "test.text.thumbnail"));
    assert!(!has_action(&pdf_actions, "core.resource.thumbnail"));
    assert!(!has_action(&pdf_actions, "azvs.markdown.read"));
    assert!(has_action(&text_actions, "test.text.extract"));
    assert!(!has_action(&text_actions, "test.text.thumbnail"));
    assert!(has_action(&text_actions, "core.resource.thumbnail"));
    assert!(!has_action(&text_actions, "azvs.markdown.read"));
}
