use super::*;

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

    let error = block_on(
        service
            .secured(&authorization, &context)
            .execute_directory_action(
                &inside.id(),
                crate::service::ExecuteDirectoryAction::new("test.directory.move")
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
fn member_cannot_replace_text_outside_the_workspace() {
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
            .with_kind(ResourceKind::try_new("core:text").unwrap())
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
