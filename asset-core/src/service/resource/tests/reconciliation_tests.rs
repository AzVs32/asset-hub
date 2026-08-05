use super::*;

#[test]
fn storage_reconciliation_creates_updates_and_removes_resources() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::from_path("external").unwrap();
    let key = StorageKey::new("external/note.txt").unwrap();
    block_on(blob_storage.ensure_directory(&directory)).unwrap();
    block_on(blob_storage.put(&key, Bytes::from_static(b"first"))).unwrap();

    block_on(service.reconcile_storage()).unwrap();
    let first = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        "note.txt",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(first.resource().content().unwrap().size(), 5);

    block_on(blob_storage.put(&key, Bytes::from_static(b"second version"))).unwrap();
    block_on(service.reconcile_storage_keys(std::slice::from_ref(&key))).unwrap();
    let updated = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        "note.txt",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(updated.resource().id(), first.resource().id());
    assert_eq!(updated.resource().content().unwrap().size(), 14);
    assert_ne!(
        updated.resource().content().unwrap().checksum(),
        first.resource().content().unwrap().checksum()
    );

    block_on(blob_storage.delete(&key)).unwrap();
    block_on(service.reconcile_storage_keys(std::slice::from_ref(&key))).unwrap();
    assert!(
        block_on(ResourceQuery::find_by_path(
            repository.as_ref(),
            &directory,
            "note.txt",
        ))
        .unwrap()
        .is_none()
    );
}

#[test]
fn startup_reconciliation_hashes_only_new_or_changed_files() {
    let (service, _, blob_storage) = service();
    let key = StorageKey::new("library/book.txt").unwrap();
    block_on(blob_storage.put(&key, Bytes::from_static(b"first"))).unwrap();

    let initial = block_on(service.reconcile_storage()).unwrap();
    assert_eq!(initial.files, 1);
    assert_eq!(initial.hashed_files, 1);
    assert_eq!(initial.unchanged_files, 0);

    let unchanged = block_on(service.reconcile_storage()).unwrap();
    assert_eq!(unchanged.hashed_files, 0);
    assert_eq!(unchanged.unchanged_files, 1);

    block_on(blob_storage.put(&key, Bytes::from_static(b"other"))).unwrap();
    let changed = block_on(service.reconcile_storage()).unwrap();
    assert_eq!(changed.hashed_files, 1);
    assert_eq!(changed.unchanged_files, 0);

    let forced = block_on(service.scan_resources()).unwrap();
    assert_eq!(forced.hashed_files, 1);
    assert_eq!(forced.unchanged_files, 0);
}

#[test]
fn storage_reconciliation_preserves_spaces_in_discovered_paths() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::from_path(" external files / project A ").unwrap();
    let name = " draft  01.txt ";
    let key = StorageKey::new(" external files / project A / draft  01.txt ").unwrap();
    block_on(blob_storage.ensure_directory(&directory)).unwrap();
    block_on(blob_storage.put(&key, Bytes::from_static(b"draft"))).unwrap();

    block_on(service.reconcile_storage()).unwrap();

    let resource = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &directory,
        name,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(resource.resource().name(), name);
    assert_eq!(
        block_on(service.locate_resource_directory(resource.resource()))
            .unwrap()
            .path(),
        &directory
    );
    assert_eq!(
        block_on(service.storage_key(resource.resource())).unwrap(),
        key
    );
    assert_eq!(
        block_on(
            service
                .content()
                .get_resource_content(&resource.resource().id())
        )
        .unwrap(),
        Some(Bytes::from_static(b"draft"))
    );
}

#[test]
fn storage_reconciliation_preserves_resource_id_on_file_rename() {
    let (service, repository, blob_storage) = service();
    let from_directory = DirectoryPath::from_path("incoming").unwrap();
    let to_directory = DirectoryPath::from_path("library").unwrap();
    let from = StorageKey::new("incoming/book.txt").unwrap();
    let to = StorageKey::new("library/renamed.txt").unwrap();
    block_on(blob_storage.ensure_directory(&from_directory)).unwrap();
    block_on(blob_storage.ensure_directory(&to_directory)).unwrap();
    block_on(blob_storage.put(&from, Bytes::from_static(b"content"))).unwrap();
    block_on(service.reconcile_storage()).unwrap();
    let original = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &from_directory,
        "book.txt",
    ))
    .unwrap()
    .unwrap();

    block_on(blob_storage.move_if_absent(&from, &to)).unwrap();
    block_on(service.reconcile_storage_rename(&from, &to)).unwrap();
    let renamed = block_on(ResourceQuery::find_by_path(
        repository.as_ref(),
        &to_directory,
        "renamed.txt",
    ))
    .unwrap()
    .unwrap();

    assert_eq!(renamed.resource().id(), original.resource().id());
    assert!(
        block_on(ResourceQuery::find_by_path(
            repository.as_ref(),
            &from_directory,
            "book.txt",
        ))
        .unwrap()
        .is_none()
    );
}

#[test]
fn storage_reconciliation_does_not_delete_resources_after_stream_failure() {
    let (service, repository, blob_storage) = service();
    let directory = DirectoryPath::root();
    let retained = StorageKey::new("a.txt").unwrap();
    let externally_deleted = StorageKey::new("b.txt").unwrap();
    block_on(blob_storage.put(&retained, Bytes::from_static(b"a"))).unwrap();
    block_on(blob_storage.put(&externally_deleted, Bytes::from_static(b"b"))).unwrap();
    block_on(service.reconcile_storage()).unwrap();

    block_on(blob_storage.delete(&externally_deleted)).unwrap();
    blob_storage.fail_scan_after_entries(1);
    assert!(block_on(service.reconcile_storage()).is_err());

    assert!(
        block_on(ResourceQuery::find_by_path(
            repository.as_ref(),
            &directory,
            "b.txt",
        ))
        .unwrap()
        .is_some()
    );
}

#[tokio::test]
async fn storage_reconciliation_waits_for_same_key_upload_to_save_resource() {
    let (service, repository, blob_storage) = service();
    let key = StorageKey::new("assets/slow.bin").unwrap();
    let (save_started, release_save) = repository.pause_next_save();
    let upload_service = service.clone();
    let upload_key = key.clone();
    let upload = tokio::spawn(async move {
        upload_service
            .upload_resource_for_test(stream_upload_command(
                "slow.bin",
                upload_key,
                Bytes::from_static(b"complete content"),
            ))
            .await
    });

    save_started
        .await
        .expect("upload should reach repository save");
    assert!(blob_storage.contains(&key));
    assert!(repository.is_empty());

    let reconcile_service = service.clone();
    let reconcile_key = key.clone();
    let reconciliation = tokio::spawn(async move {
        reconcile_service
            .reconcile_storage_keys(&[reconcile_key])
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !reconciliation.is_finished(),
        "same-key reconciliation must wait until upload saves the Resource"
    );

    release_save.send(()).unwrap();
    let uploaded = upload.await.unwrap().unwrap();
    reconciliation.await.unwrap().unwrap();

    assert_eq!(repository.len(), 1);
    assert_eq!(
        repository.find_sync(&uploaded.id()).unwrap().content(),
        uploaded.content()
    );
}
