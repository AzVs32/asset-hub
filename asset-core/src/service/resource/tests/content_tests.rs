use super::*;

#[test]
fn empty_repository_startup_recovers_metadata_before_checksum_verification() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::from_path("library").unwrap();
    let key = StorageKey::new("library/book.txt").unwrap();
    let second_key = StorageKey::new("library/second.txt").unwrap();
    block_on(blob_storage.put(&key, Bytes::from_static(b"first"))).unwrap();
    block_on(blob_storage.put(&second_key, Bytes::from_static(b"second"))).unwrap();

    let recovered = block_on(service.reconcile_storage_on_startup()).unwrap();
    assert_eq!(recovered.files, 2);
    assert_eq!(recovered.hashed_files, 0);
    assert_eq!(
        recovered.pending_verification_keys(),
        &[key.clone(), second_key.clone()]
    );

    let pending = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        "book.txt",
    ))
    .unwrap()
    .unwrap();
    let content = pending.resource().content().unwrap();
    assert_eq!(
        content.verification_status(),
        ContentVerificationStatus::Pending
    );
    assert_eq!(content.size(), 5);
    assert_eq!(content.checksum(), None);

    block_on(service.reconcile_storage_keys(std::slice::from_ref(&key))).unwrap();
    let verified = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        "book.txt",
    ))
    .unwrap()
    .unwrap();
    let content = verified.resource().content().unwrap();
    assert_eq!(
        content.verification_status(),
        ContentVerificationStatus::Verified
    );
    assert_eq!(content.checksum().unwrap().value(), hex_sha256(b"first"));

    let resumed = block_on(service.reconcile_storage_on_startup()).unwrap();
    assert_eq!(resumed.hashed_files, 0);
    assert_eq!(resumed.unchanged_files, 1);
    assert_eq!(resumed.pending_verification_keys(), &[second_key]);
}

#[test]
fn get_resource_content_reads_existing_blob() {
    let (service, _, _) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key,
        data.clone(),
    )))
    .unwrap();

    let content = block_on(service.content().get_resource_content(&resource.id())).unwrap();

    assert_eq!(content, Some(data));
}

#[test]
fn streaming_text_replacement_updates_content_and_revision() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/streamed.md").unwrap();
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("streamed.md", key.clone(), Bytes::from_static(b"# Old"))
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let replacement = Bytes::from_static(b"# Streamed\n\nUpdated.");
    let checksum = Checksum::sha256(hex_sha256(&replacement)).unwrap();
    let command = ReplaceResourceContent::new(
        replacement.len() as u64,
        checksum.clone(),
        resource.revision(),
    )
    .with_mime_type("text/markdown");

    let updated = block_on(service.content().replace_text_content_snapshot(
        repository.locate_sync(resource.clone()),
        command,
        Box::pin(futures_util::stream::once({
            let replacement = replacement.clone();
            async move { Ok(replacement) }
        })),
    ))
    .unwrap();

    assert_eq!(updated.revision(), resource.revision() + 1);
    assert_eq!(updated.content().unwrap().checksum(), Some(&checksum));
    assert_eq!(blob_storage.get_sync(&key), Some(replacement));
    assert_eq!(
        repository.find_sync(&resource.id()).unwrap().revision(),
        updated.revision()
    );
}

#[tokio::test]
async fn streaming_text_replacement_serializes_with_rename_on_the_same_blob() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/concurrent.md").unwrap();
    let renamed_key = StorageKey::new("docs/renamed.md").unwrap();
    let resource = service
        .upload_resource_for_test(
            stream_upload_command(
                "concurrent.md",
                key.clone(),
                Bytes::from_static(b"# Original"),
            )
            .with_kind(ResourceKind::try_new("core:text").unwrap())
            .with_mime_type("text/markdown"),
        )
        .await
        .unwrap();
    let replacement = Bytes::from_static(b"# Replacement");
    let command = ReplaceResourceContent::new(
        replacement.len() as u64,
        Checksum::sha256(hex_sha256(&replacement)).unwrap(),
        resource.revision(),
    );
    let (save_started, release_save) = repository.pause_next_save();

    let replacement_service = service.clone();
    let replacement_snapshot = repository.locate_sync(resource.clone());
    let replacement_task = tokio::spawn(async move {
        replacement_service
            .content()
            .replace_text_content_snapshot(
                replacement_snapshot,
                command,
                Box::pin(futures_util::stream::once(async move { Ok(replacement) })),
            )
            .await
    });
    save_started
        .await
        .expect("replacement should reach its conditional Resource save");

    let rename_service = service.clone();
    let rename_snapshot = repository.locate_sync(resource.clone());
    let rename_revision = rename_snapshot.resource().revision();
    let rename_task = tokio::spawn(async move {
        rename_service
            .commands()
            .update_resource_snapshot(
                rename_snapshot,
                UpdateResource::new(rename_revision).with_name("renamed.md"),
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !rename_task.is_finished(),
        "rename must wait while replacement owns the original Blob key"
    );

    release_save.send(()).unwrap();
    let updated = replacement_task.await.unwrap().unwrap();
    let rename_error = rename_task.await.unwrap().unwrap_err();

    assert!(matches!(rename_error, CoreError::Conflict { .. }));
    assert_eq!(
        blob_storage.get_sync(&key),
        Some(Bytes::from_static(b"# Replacement"))
    );
    assert!(!blob_storage.contains(&renamed_key));
    assert_eq!(
        repository.find_sync(&resource.id()).unwrap().revision(),
        updated.revision()
    );
}

#[test]
fn streaming_text_replacement_restores_the_original_blob_when_cas_fails() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/rollback.md").unwrap();
    let original = Bytes::from_static(b"# Original");
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("rollback.md", key.clone(), original.clone())
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let replacement = Bytes::from_static(b"# Replacement");
    let command = ReplaceResourceContent::new(
        replacement.len() as u64,
        Checksum::sha256(hex_sha256(&replacement)).unwrap(),
        resource.revision(),
    );
    repository.fail_next_conditional_save();

    let error = block_on(service.content().replace_text_content_snapshot(
        repository.locate_sync(resource.clone()),
        command,
        Box::pin(futures_util::stream::once(async move { Ok(replacement) })),
    ))
    .unwrap_err();

    assert!(matches!(error, CoreError::Repository { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(original));
    assert_eq!(
        repository.find_sync(&resource.id()).unwrap().revision(),
        resource.revision()
    );
}

#[test]
fn startup_recovery_rolls_back_a_published_replacement_without_a_resource_commit() {
    let (service, repository, blob_storage) = service();
    let target = StorageKey::new("docs/interrupted.md").unwrap();
    let original = Bytes::from_static(b"# Original");
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("interrupted.md", target.clone(), original.clone())
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let replacement_bytes = Bytes::from_static(b"# Interrupted replacement");
    let replacement_content = crate::domain::ResourceContent::verified(
        replacement_bytes.len() as u64,
        Checksum::sha256(hex_sha256(&replacement_bytes)).unwrap(),
    )
    .with_mime_type("text/markdown")
    .build()
    .unwrap();
    let id = ResourceContentReplacementId::new();
    let staged = StorageKey::new(format!(".asset-hub/uploads/replacement-{id}")).unwrap();
    let backup = StorageKey::new(format!(".asset-hub/content-backups/{id}")).unwrap();
    let pending = ResourceContentReplacement::rehydrate(
        id,
        resource.id(),
        resource.revision(),
        target.clone(),
        staged.clone(),
        backup.clone(),
        replacement_content,
    )
    .unwrap();
    block_on(service.content_replacements.save(&pending)).unwrap();
    block_on(blob_storage.put(&staged, replacement_bytes.clone())).unwrap();
    block_on(blob_storage.move_if_absent(&target, &backup)).unwrap();
    block_on(blob_storage.put(&target, replacement_bytes)).unwrap();

    assert_eq!(block_on(service.resume_content_replacements()).unwrap(), 1);

    assert_eq!(blob_storage.get_sync(&target), Some(original));
    assert!(!blob_storage.contains(&backup));
    assert!(!blob_storage.contains(&staged));
    assert_eq!(repository.find_sync(&resource.id()).unwrap(), resource);
    assert!(
        block_on(service.content_replacements.list_pending())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn startup_recovery_keeps_a_committed_replacement_and_cleans_internal_blobs() {
    let (service, repository, blob_storage) = service();
    let target = StorageKey::new("docs/committed.md").unwrap();
    let original = Bytes::from_static(b"# Original");
    let mut resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("committed.md", target.clone(), original)
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let expected_revision = resource.revision();
    let replacement_bytes = Bytes::from_static(b"# Committed replacement");
    let replacement_content = crate::domain::ResourceContent::verified(
        replacement_bytes.len() as u64,
        Checksum::sha256(hex_sha256(&replacement_bytes)).unwrap(),
    )
    .with_mime_type("text/markdown")
    .build()
    .unwrap();
    let id = ResourceContentReplacementId::new();
    let staged = StorageKey::new(format!(".asset-hub/uploads/replacement-{id}")).unwrap();
    let backup = StorageKey::new(format!(".asset-hub/content-backups/{id}")).unwrap();
    let pending = ResourceContentReplacement::rehydrate(
        id,
        resource.id(),
        expected_revision,
        target.clone(),
        staged.clone(),
        backup.clone(),
        replacement_content.clone(),
    )
    .unwrap();
    block_on(service.content_replacements.save(&pending)).unwrap();
    block_on(blob_storage.put(&staged, replacement_bytes.clone())).unwrap();
    block_on(blob_storage.move_if_absent(&target, &backup)).unwrap();
    block_on(blob_storage.put(&target, replacement_bytes.clone())).unwrap();
    resource.attach_content(replacement_content).unwrap();
    block_on(repository.save(&resource)).unwrap();

    assert_eq!(block_on(service.resume_content_replacements()).unwrap(), 1);

    assert_eq!(blob_storage.get_sync(&target), Some(replacement_bytes));
    assert!(!blob_storage.contains(&backup));
    assert!(!blob_storage.contains(&staged));
    assert!(
        block_on(service.content_replacements.list_pending())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn streaming_text_replacement_rejects_bad_checksums_and_oversized_content() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/validated.md").unwrap();
    let original = Bytes::from_static(b"# Original");
    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("validated.md", key.clone(), original.clone())
                .with_kind(ResourceKind::try_new("core:text").unwrap())
                .with_mime_type("text/markdown"),
        ),
    )
    .unwrap();
    let replacement = Bytes::from_static(b"changed");
    let bad_checksum = Checksum::sha256("0".repeat(64)).unwrap();
    let error = block_on(service.content().replace_text_content_snapshot(
        repository.locate_sync(resource.clone()),
        ReplaceResourceContent::new(replacement.len() as u64, bad_checksum, resource.revision()),
        Box::pin(futures_util::stream::once(async move { Ok(replacement) })),
    ))
    .unwrap_err();
    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(original.clone()));

    let max = test_resource_content_edit_policy().max_text_bytes();
    let error = block_on(service.content().replace_text_content_snapshot(
        repository.locate_sync(resource.clone()),
        ReplaceResourceContent::new(
            max + 1,
            Checksum::sha256("0".repeat(64)).unwrap(),
            resource.revision(),
        ),
        Box::pin(futures_util::stream::empty()),
    ))
    .unwrap_err();
    assert!(matches!(error, CoreError::LimitExceeded { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(original));
}

#[test]
fn text_edit_capability_is_hidden_above_the_edit_policy() {
    let (service, _, _) = service();
    let resource = Resource::builder("large.md")
        .with_kind(ResourceKind::try_new("core:text").unwrap())
        .with_content(
            crate::domain::ResourceContent::verified(
                test_resource_content_edit_policy().max_text_bytes() + 1,
                Checksum::sha256("0".repeat(64)).unwrap(),
            )
            .with_mime_type("text/markdown")
            .build()
            .unwrap(),
        )
        .build()
        .unwrap();

    let actions = service.describe_resource_actions(&resource).unwrap();
    assert!(actions.available_actions().iter().all(|action| {
        !action
            .provides()
            .is_some_and(|capability| capability.as_str() == "text_edit")
    }));
}
