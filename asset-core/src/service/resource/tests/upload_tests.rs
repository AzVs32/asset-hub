use super::*;

#[test]
fn stream_upload_resource_content_writes_blob_then_saves_resource() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let checksum = Checksum::sha256(hex_sha256(&data)).unwrap();

    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("image", key.clone(), data.clone())
                .with_kind(ResourceKind::try_new("core:image").unwrap())
                .with_mime_type(" image/png "),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();
    let content = saved.content().unwrap();

    assert_eq!(block_on(service.storage_key(&saved)).unwrap(), key);
    assert_eq!(content.size(), data.len() as u64);
    assert_eq!(content.mime_type(), Some("image/png"));
    assert_eq!(content.checksum(), Some(&checksum));
    assert_eq!(
        content.modified_at(),
        blob_storage.modified_at.lock().unwrap().get(&key).copied()
    );
    assert_eq!(blob_storage.get_sync(&key), Some(data));

    let startup = block_on(service.reconcile_storage()).unwrap();
    assert_eq!(startup.hashed_files, 0);
    assert_eq!(startup.unchanged_files, 1);
}

#[test]
fn stream_upload_preserves_spaces_in_resource_and_blob_path() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::from_path(" library / project A ").unwrap();
    let name = " design  draft 01.md ";
    let key = StorageKey::new(" library / project A / design  draft 01.md ").unwrap();
    let data = Bytes::from_static(b"draft");
    let stream = futures_util::stream::once({
        let data = data.clone();
        async move { Ok(data) }
    });

    let resource = block_on(
        service.upload_resource_for_test(
            TestUpload::new(name, Box::pin(stream))
                .with_directory(directory.clone())
                .with_kind(ResourceKind::try_new("azvs:markdown").unwrap()),
        ),
    )
    .unwrap();

    assert_eq!(resource.name(), name);
    assert_eq!(
        block_on(service.locate_resource_directory(&resource))
            .unwrap()
            .path(),
        &directory
    );
    assert_eq!(block_on(service.storage_key(&resource)).unwrap(), key);
    assert_eq!(
        block_on(service.storage_key(&repository.find_sync(&resource.id()).unwrap())).unwrap(),
        key
    );
    assert_eq!(
        blob_storage.get_sync(&key),
        Some(Bytes::from_static(b"draft"))
    );
}

#[test]
fn stream_upload_resource_content_detects_most_specific_kind() {
    let (service, repository, _) = service();
    let key = StorageKey::new("docs/readme.md").unwrap();

    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("readme", key, Bytes::from_static(b"# Readme"))
                .with_mime_type("text/plain"),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();

    assert!(saved.kind().is("azvs:markdown"));
}

#[test]
fn stream_upload_resource_content_falls_back_to_core_resource() {
    let (service, repository, _) = service();
    let key = StorageKey::new("assets/archive.bin").unwrap();

    let resource = block_on(
        service.upload_resource_for_test(
            stream_upload_command("archive", key, Bytes::from_static(b"binary"))
                .with_mime_type("application/octet-stream"),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();
    assert_eq!(saved.kind(), &ResourceKind::default());
}

#[test]
fn stream_upload_resource_content_rejects_existing_storage_key() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    blob_storage
        .objects
        .lock()
        .unwrap()
        .insert(key.clone(), Bytes::from_static(b"existing"));
    blob_storage
        .modified_at
        .lock()
        .unwrap()
        .insert(key.clone(), chrono::Utc::now());

    let error = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key,
        Bytes::from_static(b"new"),
    )))
    .unwrap_err();

    match error {
        CoreError::Conflict { message } => assert!(message.contains("already exists")),
        other => panic!("expected storage key conflict, got {other:?}"),
    }
    assert!(repository.is_empty());
    assert_eq!(
        blob_storage.get_sync(&StorageKey::new("assets/image.png").unwrap()),
        Some(Bytes::from_static(b"existing"))
    );
    assert!(blob_storage.contains_fragment(".asset-hub/uploads/"));
}

#[test]
fn stream_upload_rejects_unsupported_kind() {
    let (service, repository, _) = service_with_registry(Arc::new(
        InMemoryResourceKindRegistry::with_definitions(vec![ResourceKindDefinition::new(
            ResourceKind::default(),
            "Unknown",
            true,
        )]),
    ));

    let error = block_on(
        service.upload_resource_for_test(
            stream_upload_command(
                "image",
                StorageKey::new("image.png").unwrap(),
                Bytes::from_static(b"image"),
            )
            .with_kind(ResourceKind::try_new("plugin:not-installed").unwrap()),
        ),
    )
    .unwrap_err();

    match error {
        CoreError::Unsupported { subject, value } => {
            assert_eq!(subject, "resource kind");
            assert_eq!(value, "plugin:not-installed");
        }
        other => panic!("expected unsupported error, got {other:?}"),
    }
    assert!(repository.is_empty());
}

#[test]
fn stream_create_resource_writes_chunks_and_records_size() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/large.bin").unwrap();
    let data: BlobByteStream = Box::pin(futures_util::stream::iter([
        Ok(Bytes::from_static(b"large ")),
        Ok(Bytes::from_static(b"file ")),
        Ok(Bytes::from_static(b"bytes")),
    ]));

    let resource = block_on(
        service.upload_resource_for_test(
            TestUpload::new("large.bin", data)
                .with_directory(DirectoryPath::from_path("assets").unwrap())
                .with_kind(ResourceKind::try_new("asset:binary").unwrap())
                .with_mime_type("application/octet-stream"),
        ),
    )
    .unwrap();

    let saved = repository.find_sync(&resource.id()).unwrap();
    let content = saved.content().unwrap();

    assert_eq!(block_on(service.storage_key(&saved)).unwrap(), key);
    assert_eq!(content.size(), 16);
    assert_eq!(content.mime_type(), Some("application/octet-stream"));
    assert_eq!(
        blob_storage.get_sync(&key),
        Some(Bytes::from_static(b"large file bytes"))
    );
    assert!(!blob_storage.contains_fragment(".asset-hub/uploads/"));
}

#[tokio::test]
async fn pending_upload_finalization_is_exposed_for_runtime_scheduling() {
    let (service, _repository, blob_storage) = service();
    let owner = UserId::new();
    let data = Bytes::from_static(b"resume finalization");
    let session = service
        .uploads()
        .create(
            owner,
            CreateUpload::new(
                "resumed.bin",
                data.len() as u64,
                Checksum::sha256(hex_sha256(&data)).unwrap(),
            )
            .with_directory(DirectoryPath::from_path("assets").unwrap()),
        )
        .await
        .unwrap();
    service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(&data)).unwrap(),
            Box::pin(futures_util::stream::once({
                let data = data.clone();
                async move { Ok(data) }
            })),
        )
        .await
        .unwrap();
    let (finalizing, should_start) = service
        .uploads()
        .request_finalization(owner, &session.id())
        .await
        .unwrap();
    assert!(should_start);
    assert_eq!(finalizing.status(), UploadStatus::Finalizing);

    assert_eq!(
        service.pending_upload_finalizations().await.unwrap(),
        vec![session.id()]
    );
    let resource = service.finalize_upload(&session.id()).await.unwrap();

    assert_eq!(resource.id(), session.resource_id());
    assert_eq!(
        blob_storage.get_sync(&StorageKey::new("assets/resumed.bin").unwrap()),
        Some(data)
    );
}

#[test]
fn stream_upload_resource_content_rejects_kind_without_content_support() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("docs/readme.md").unwrap();

    let error = block_on(
        service.upload_resource_for_test(
            stream_upload_command("readme", key.clone(), Bytes::from_static(b"hello"))
                .with_kind(ResourceKind::try_new("doc:markdown").unwrap()),
        ),
    )
    .unwrap_err();

    match error {
        CoreError::Unsupported { subject, value } => {
            assert_eq!(subject, "resource kind for content upload");
            assert_eq!(value, "doc:markdown");
        }
        other => panic!("expected unsupported error, got {other:?}"),
    }
    assert!(repository.is_empty());
    assert!(!blob_storage.contains(&key));
}

#[test]
fn stream_upload_resource_content_removes_blob_when_save_fails() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    repository.fail_next_save();

    let result = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key.clone(),
        Bytes::from_static(b"image bytes"),
    )));

    match result {
        Err(CoreError::Repository { operation, .. }) => assert_eq!(operation, "save"),
        other => panic!("expected repository error, got {other:?}"),
    }

    assert!(!blob_storage.contains(&key));
    assert!(blob_storage.contains_fragment(".asset-hub/uploads/"));
    assert!(repository.is_empty());
}

#[test]
fn upload_preserves_repository_error_when_compensation_delete_fails() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/compensation.bin").unwrap();
    repository.fail_next_save();
    blob_storage.fail_delete_for(key.clone());

    let error = block_on(service.upload_resource_for_test(stream_upload_command(
        "file",
        key.clone(),
        Bytes::from_static(b"data"),
    )))
    .unwrap_err();

    assert!(matches!(
        error,
        CoreError::Repository {
            operation: "save",
            ..
        }
    ));
    assert!(blob_storage.contains(&key));
    assert!(blob_storage.contains_fragment(".asset-hub/uploads/"));
}

#[tokio::test]
async fn upload_chunk_checksum_mismatch_keeps_offset_and_staged_content_unchanged() {
    let (service, _repository, blob_storage) = service();
    let owner = UserId::new();
    let data = Bytes::from_static(b"verified chunk");
    let session = service
        .uploads()
        .create(
            owner,
            CreateUpload::new(
                "chunk.bin",
                data.len() as u64,
                Checksum::sha256(hex_sha256(&data)).unwrap(),
            ),
        )
        .await
        .unwrap();

    let error = service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(b"different chunk")).unwrap(),
            Box::pin(futures_util::stream::once({
                let data = data.clone();
                async move { Ok(data) }
            })),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("chunk checksum mismatch"));
    let status = service
        .uploads()
        .status(owner, &session.id())
        .await
        .unwrap();
    assert_eq!(status.offset(), 0);
    let staged_key = StorageKey::new(format!(".asset-hub/uploads/{}", session.id())).unwrap();
    assert_eq!(blob_storage.get_sync(&staged_key), Some(Bytes::new()));
    assert!(!blob_storage.contains_fragment(".chunk"));

    let resumed = service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(&data)).unwrap(),
            Box::pin(futures_util::stream::once(async move { Ok(data) })),
        )
        .await
        .unwrap();
    assert_eq!(resumed.offset(), resumed.expected_size());
    assert_eq!(
        blob_storage.get_sync(&staged_key),
        Some(Bytes::from_static(b"verified chunk"))
    );
    assert!(!blob_storage.contains_fragment(".chunk"));
}

#[tokio::test]
async fn interrupted_upload_chunk_never_reaches_the_session_staging_file() {
    let (service, _repository, blob_storage) = service();
    let owner = UserId::new();
    let session = service
        .uploads()
        .create(
            owner,
            CreateUpload::new(
                "interrupted.bin",
                12,
                Checksum::sha256(hex_sha256(b"partial data")).unwrap(),
            ),
        )
        .await
        .unwrap();
    let stream = futures_util::stream::iter(vec![
        Ok(Bytes::from_static(b"partial")),
        Err(CoreError::conflict("request body interrupted")),
    ]);

    let error = service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(b"partial data")).unwrap(),
            Box::pin(stream),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("request body interrupted"));
    let status = service
        .uploads()
        .status(owner, &session.id())
        .await
        .unwrap();
    assert_eq!(status.offset(), 0);
    let staged_key = StorageKey::new(format!(".asset-hub/uploads/{}", session.id())).unwrap();
    assert_eq!(blob_storage.get_sync(&staged_key), Some(Bytes::new()));
    assert!(!blob_storage.contains_fragment(".chunk"));
}

#[tokio::test]
async fn upload_checksum_mismatch_fails_before_publication() {
    let (service, repository, blob_storage) = service();
    let owner = UserId::new();
    let expected = Bytes::from_static(b"right");
    let received = Bytes::from_static(b"wrong");
    let key = StorageKey::new("assets/mismatch.bin").unwrap();
    let session = service
        .uploads()
        .create(
            owner,
            CreateUpload::new(
                "mismatch.bin",
                received.len() as u64,
                Checksum::sha256(hex_sha256(&expected)).unwrap(),
            )
            .with_directory(DirectoryPath::from_path("assets").unwrap()),
        )
        .await
        .unwrap();
    service
        .uploads()
        .append(
            owner,
            &session.id(),
            0,
            Checksum::sha256(hex_sha256(&received)).unwrap(),
            Box::pin(futures_util::stream::once(async move { Ok(received) })),
        )
        .await
        .unwrap();
    service
        .uploads()
        .request_finalization(owner, &session.id())
        .await
        .unwrap();

    let error = service.uploads().finalize(&session.id()).await.unwrap_err();
    assert!(error.to_string().contains("checksum mismatch"));
    let failed = service
        .uploads()
        .status(owner, &session.id())
        .await
        .unwrap();
    assert_eq!(failed.status(), UploadStatus::Failed);
    assert_eq!(
        failed.actual_checksum().unwrap().value(),
        hex_sha256(b"wrong")
    );
    assert!(repository.find_sync(&session.resource_id()).is_none());
    assert!(!blob_storage.contains(&key));
    assert!(blob_storage.contains_fragment(".asset-hub/uploads/"));
}
