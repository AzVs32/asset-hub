use super::*;

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
