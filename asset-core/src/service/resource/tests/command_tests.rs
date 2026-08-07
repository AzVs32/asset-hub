use super::*;

#[test]
fn update_resource_rejects_a_stale_authorized_snapshot() {
    let (service, repository, _) = service();
    let resource = command::build_resource(
        "original".to_string(),
        DirectoryId::root(),
        Some(ResourceKind::try_new("doc:markdown").unwrap()),
    )
    .build()
    .unwrap();
    block_on(repository.save(&resource)).unwrap();
    let stale = resource.clone();
    let mut concurrent = resource;
    concurrent.rename("concurrent").unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(service.commands().update_resource_snapshot(
        repository.locate_sync(stale),
        UpdateResource::new().with_name("stale"),
    ))
    .unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(
        repository.find_sync(&concurrent.id()).unwrap().name(),
        "concurrent"
    );
}

#[test]
fn soft_delete_resource_moves_blob_to_trash_and_hides_content_read() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key.clone(),
        data.clone(),
    )))
    .unwrap();
    let trash_key = StorageKey::new(format!(".asset-hub/trash/{}", resource.id())).unwrap();

    let deleted = block_on(service.commands().soft_delete_resource(&resource.id()))
        .unwrap()
        .unwrap();
    let content = block_on(service.content().get_resource_content(&resource.id())).unwrap();

    assert!(deleted.is_deleted());
    assert!(repository.find_sync(&resource.id()).unwrap().is_deleted());
    assert!(!blob_storage.contains(&key));
    assert_eq!(blob_storage.get_sync(&trash_key), Some(data));
    assert!(content.is_none());
}

#[test]
fn restoring_soft_deleted_resource_moves_blob_back_from_trash() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/restored.png").unwrap();
    let data = Bytes::from_static(b"restored bytes");
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "restored",
        key.clone(),
        data.clone(),
    )))
    .unwrap();
    let trash_key = StorageKey::new(format!(".asset-hub/trash/{}", resource.id())).unwrap();
    let deleted = block_on(service.commands().soft_delete_resource(&resource.id()))
        .unwrap()
        .unwrap();

    let restored = block_on(service.commands().update_resource_snapshot(
        repository.locate_sync(deleted),
        UpdateResource::new().with_restore(true),
    ))
    .unwrap();

    assert!(!restored.is_deleted());
    assert_eq!(blob_storage.get_sync(&key), Some(data));
    assert!(!blob_storage.contains(&trash_key));
}

#[test]
fn soft_delete_rolls_blob_back_when_resource_snapshot_is_stale() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/concurrent-delete.png").unwrap();
    let data = Bytes::from_static(b"still active");
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "concurrent-delete",
        key.clone(),
        data.clone(),
    )))
    .unwrap();
    let stale = resource.clone();
    let trash_key = StorageKey::new(format!(".asset-hub/trash/{}", resource.id())).unwrap();
    let mut concurrent = resource;
    concurrent
        .change_kind(ResourceKind::try_new("core:image").unwrap())
        .unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(
        service
            .commands()
            .soft_delete_resource_snapshot(repository.locate_sync(stale)),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(blob_storage.get_sync(&key), Some(data));
    assert!(!blob_storage.contains(&trash_key));
    assert!(!repository.find_sync(&concurrent.id()).unwrap().is_deleted());
}

#[test]
fn remove_resource_deletes_blob_and_repository_record() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/image.png").unwrap();
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key.clone(),
        Bytes::from_static(b"image bytes"),
    )))
    .unwrap();

    assert!(block_on(service.commands().remove_resource(&resource.id())).unwrap());
    assert!(repository.find_sync(&resource.id()).is_none());
    assert!(!blob_storage.contains(&key));
    assert!(!block_on(service.commands().remove_resource(&resource.id())).unwrap());
}

#[test]
fn remove_resource_rejects_a_stale_authorized_snapshot_without_deleting_content() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/concurrent.png").unwrap();
    let resource = block_on(service.upload_resource_for_test(stream_upload_command(
        "image",
        key.clone(),
        Bytes::from_static(b"image bytes"),
    )))
    .unwrap();
    let stale = resource.clone();
    let mut concurrent = resource;
    concurrent.rename("moved by another request").unwrap();
    block_on(repository.save(&concurrent)).unwrap();

    let error = block_on(
        service
            .commands()
            .remove_resource_snapshot(repository.locate_sync(stale)),
    )
    .unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert!(repository.find_sync(&concurrent.id()).is_some());
    assert!(blob_storage.contains(&key));
}
