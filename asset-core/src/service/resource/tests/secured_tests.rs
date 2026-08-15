use super::*;
use crate::domain::DirectoryActionId;

#[test]
fn directory_action_cannot_move_a_directory_outside_the_member_workspace() {
    let (service, _, _) = service();
    let root = block_on(service.directory_service().root()).unwrap();
    let workspace = block_on(service.directory_service().create(&root, "workspace")).unwrap();
    let outside = block_on(service.directory_service().create(&root, "outside")).unwrap();
    let inside = block_on(service.directory_service().create(&workspace, "inside")).unwrap();
    let user = User::new("member", "hash", UserRole::Member, workspace.id()).unwrap();
    let context = AccessContext::member(user.id());
    let authorization = crate::service::AuthorizationService::new(
        Arc::new(SingleUserRepository(user)),
        service.directory_service().clone(),
    );
    let inside_revision = block_on(service.directory_service().find_by_id(&inside.id()))
        .unwrap()
        .directory()
        .revision();

    let error = block_on(
        service
            .directory_service()
            .secured(&authorization, &context)
            .execute_action(
                &inside.id(),
                crate::service::ExecuteDirectoryAction::new(
                    DirectoryActionId::from_static("test.directory.move"),
                    None,
                )
                .with_input(serde_json::json!({"parent_id": outside.id().to_string()})),
            ),
    )
    .unwrap_err();
    assert!(matches!(error, CoreError::InvalidOperation { .. }));

    let error = block_on(
        service
            .directory_service()
            .secured(&authorization, &context)
            .execute_action(
                &inside.id(),
                crate::service::ExecuteDirectoryAction::new(
                    DirectoryActionId::from_static("test.directory.move"),
                    Some(inside_revision),
                )
                .with_input(serde_json::json!({"parent_id": outside.id().to_string()})),
            ),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::Forbidden { .. }));
    assert_eq!(
        block_on(service.directory_service().locate_by_id(&inside.id()))
            .unwrap()
            .path()
            .path(),
        "workspace/inside"
    );
}

#[test]
fn member_cannot_replace_content_outside_the_workspace() {
    let (service, _, _) = service();
    let root = block_on(service.directory_service().root()).unwrap();
    let workspace = block_on(service.directory_service().create(&root, "workspace")).unwrap();
    let outside = block_on(service.directory_service().create(&root, "outside")).unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "outside.md",
                StorageKey::new("outside/outside.md").unwrap(),
                Bytes::from_static(b"outside"),
            )
            .with_kind(ResourceKind::try_new("example:document").unwrap())
            .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    assert_eq!(resource.directory_id(), outside.id());
    let user = User::new("member", "hash", UserRole::Member, workspace.id()).unwrap();
    let context = AccessContext::member(user.id());
    let authorization = crate::service::AuthorizationService::new(
        Arc::new(SingleUserRepository(user)),
        service.directory_service().clone(),
    );
    let replacement = Bytes::from_static(b"denied");
    let command = ReplaceResourceContent::new(
        replacement.len() as u64,
        Checksum::sha256(hex_sha256(&replacement)).unwrap(),
        resource.revision(),
    );

    let error = block_on(
        service
            .secured(&authorization, &context)
            .replace_resource_content(
                &resource.id(),
                command,
                Box::pin(futures_util::stream::once(async move { Ok(replacement) })),
            ),
    )
    .unwrap_err();
    assert!(matches!(error, CoreError::Forbidden { .. }));
}

#[test]
fn secured_directory_action_creates_a_bounded_directory_and_resource_tree() {
    let (service, repository, blob_storage) = service();
    let root = block_on(service.directory_service().root()).unwrap();
    let workspace = block_on(service.directory_service().create(&root, "workspace")).unwrap();
    let user = User::new("member", "hash", UserRole::Member, workspace.id()).unwrap();
    let context = AccessContext::member(user.id());
    let authorization = crate::service::AuthorizationService::new(
        Arc::new(SingleUserRepository(user)),
        service.directory_service().clone(),
    );
    let revision = block_on(service.directory_service().find_by_id(&workspace.id()))
        .unwrap()
        .directory()
        .revision();

    block_on(
        service
            .secured(&authorization, &context)
            .execute_directory_action(
                &workspace.id(),
                crate::service::ExecuteDirectoryAction::new(
                    DirectoryActionId::from_static("test.directory.scaffold"),
                    Some(revision),
                ),
            ),
    )
    .unwrap();

    let game_path = DirectoryPath::from_path("workspace/game-one").unwrap();
    let public_path = DirectoryPath::from_path("workspace/game-one/public").unwrap();
    assert!(
        block_on(service.directory_service().find_by_path(&game_path)).is_ok(),
        "game directory must exist"
    );
    assert!(
        block_on(service.directory_service().find_by_path(&public_path)).is_ok(),
        "public directory must exist"
    );
    let readme = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &game_path,
        "README.md",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(readme.resource().kind().as_str(), "example:document");
    assert_eq!(
        blob_storage
            .get_sync(&StorageKey::new("workspace/game-one/README.md").unwrap())
            .unwrap(),
        Bytes::from_static(b"# Game One\n")
    );
    let hash = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &game_path,
        "HASH.md",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(hash.resource().kind().as_str(), "example:document");
    assert_eq!(
        blob_storage
            .get_sync(&StorageKey::new("workspace/game-one/HASH.md").unwrap())
            .unwrap(),
        Bytes::new()
    );
    let staging_keys = blob_storage.created_staging_keys();
    assert_eq!(staging_keys.len(), 2);
    assert!(
        staging_keys
            .iter()
            .all(|key| { key.as_str().starts_with(".asset-hub/uploads/generated-") })
    );
}
