use super::*;
use asset_core::{
    directory::{
        domain::{Directory, DirectoryPath},
        port::DirectoryRevisionUpdate,
    },
    idempotency::domain::IdempotencyKey,
    resource::{
        domain::{
            Checksum, Resource, ResourceContent, ResourceContentReplacement, ResourceDeletion,
            UploadStatus,
        },
        port::ResourceRelocation,
        query::ListResources,
    },
    storage::{
        StorageKey,
        port::{BlobByteStream, ContentReader},
    },
};
use asset_infra::AssetInfrastructure;
use asset_infra::config::{
    BlobConfig, DatabaseConfig, LocalBlobConfig, LocalBlobSyncConfig, SqliteDatabaseConfig,
};
use bytes::Bytes;
use futures_util::TryStreamExt;
use std::time::Duration;

fn recovery_config(root: std::path::PathBuf) -> AssetInfraConfig {
    AssetInfraConfig {
        database: DatabaseConfig {
            sqlite: SqliteDatabaseConfig { max_connections: 4 },
            ..DatabaseConfig::default()
        },
        blob: BlobConfig {
            local: LocalBlobConfig {
                root,
                sync: LocalBlobSyncConfig {
                    enabled: false,
                    ..LocalBlobSyncConfig::default()
                },
            },
            ..BlobConfig::default()
        },
        ..AssetInfraConfig::default()
    }
}

async fn recovery_environment(
    name: &str,
) -> (std::path::PathBuf, AssetRuntime, AssetInfrastructure) {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("asset-hub-{name}-{}-{nonce}", std::process::id()));
    let config = recovery_config(root.clone());
    let runtime = AssetRuntime::new(config.clone()).await.unwrap();
    let infrastructure = AssetInfrastructure::new(config).await.unwrap();
    (root, runtime, infrastructure)
}

#[tokio::test]
async fn runtime_uses_the_configured_idempotency_lease_duration() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "asset-hub-idempotency-config-{}-{nonce}",
        std::process::id()
    ));
    let mut config = recovery_config(root.clone());
    config.idempotency.lease_duration_seconds = 1;

    let runtime = AssetRuntime::new(config).await.unwrap();
    assert_eq!(
        runtime.idempotency_service().lease_duration(),
        Duration::from_secs(1)
    );

    drop(runtime);
    let _ = std::fs::remove_dir_all(root);
}

fn verified_content(size: u64) -> ResourceContent {
    ResourceContent::verified(size, Checksum::sha256("0".repeat(64)).unwrap())
        .build()
        .unwrap()
}

fn write(root: &std::path::Path, key: &StorageKey, bytes: &[u8]) {
    let path = root.join(key.as_str());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

async fn read_all(reader: &dyn ContentReader, key: &StorageKey) -> Option<Bytes> {
    let chunks = reader
        .get_stream(key)
        .await
        .unwrap()?
        .try_collect::<Vec<_>>()
        .await
        .unwrap();
    let mut data = Vec::with_capacity(chunks.iter().map(Bytes::len).sum());
    for chunk in chunks {
        data.extend_from_slice(&chunk);
    }
    Some(Bytes::from(data))
}

fn upload_stream(bytes: &'static [u8]) -> BlobByteStream {
    Box::pin(futures_util::stream::iter([Ok(Bytes::from_static(bytes))]))
}

#[tokio::test]
async fn rejected_resource_rename_does_not_leave_a_startup_blocking_intent() {
    let (root, runtime, infrastructure) = recovery_environment("rename-occupied").await;
    let resource = Resource::builder("source.txt")
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    std::fs::write(root.join("source.txt"), b"old").unwrap();
    // A local file can exist before filesystem synchronization imports it.
    std::fs::write(root.join("occupied.txt"), b"unrelated").unwrap();
    let result = runtime
        .resource_service()
        .update(
            &resource.id(),
            asset_core::resource::service::UpdateResource::new(resource.revision())
                .with_name("occupied.txt"),
        )
        .await;
    assert!(matches!(
        result,
        Err(asset_core::CoreError::Conflict { .. })
    ));
    assert!(
        infrastructure
            .resource_relocation_store()
            .load_all()
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(std::fs::read(root.join("source.txt")).unwrap(), b"old");
    assert_eq!(
        std::fs::read(root.join("occupied.txt")).unwrap(),
        b"unrelated"
    );
    drop(infrastructure);
    drop(runtime);
    let restarted = AssetRuntime::new(recovery_config(root.clone()))
        .await
        .unwrap();
    drop(restarted);
    std::fs::remove_dir_all(root).unwrap();
}

// Prevent existence checks from silently trimming a valid path before relocation or deletion.
#[tokio::test]
async fn spaced_resource_paths_survive_rename_and_are_physically_deleted() {
    let (root, runtime, infrastructure) = recovery_environment("spaced-resource").await;
    let resource = Resource::builder(" spaced.txt ")
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    std::fs::write(root.join(" spaced.txt "), b"old").unwrap();
    std::fs::write(root.join("renamed.txt"), b"independent").unwrap();
    let renamed = runtime
        .resource_service()
        .update(
            &resource.id(),
            asset_core::resource::service::UpdateResource::new(resource.revision())
                .with_name(" renamed.txt "),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(!root.join(" spaced.txt ").exists());
    assert_eq!(std::fs::read(root.join(" renamed.txt ")).unwrap(), b"old");
    assert!(
        runtime
            .resource_service()
            .delete(&renamed.id(), renamed.revision())
            .await
            .unwrap()
    );
    assert!(!root.join(" renamed.txt ").exists());
    assert!(
        runtime
            .resource_service()
            .get(&renamed.id())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        std::fs::read(root.join("renamed.txt")).unwrap(),
        b"independent"
    );
    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

// Prevent a chunk-local UTF-8 check, or validation after publication, from damaging text edits.
#[tokio::test]
async fn text_replacement_validates_utf8_across_chunks_before_publication() {
    use asset_core::resource::service::ReplaceResourceContent;
    use sha2::{Digest, Sha256};
    let (root, runtime, infrastructure) = recovery_environment("utf8-replacement").await;
    let resource = Resource::builder("note.txt")
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    std::fs::write(root.join("note.txt"), b"old").unwrap();
    // First reject a malformed continuation after a valid prefix, then an incomplete final code point.
    for bytes in [b"prefix\xe4x".as_slice(), b"prefix\xf0\x9f".as_slice()] {
        let checksum = Checksum::sha256(
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
        .unwrap();
        let result = runtime
            .content_service()
            .replace(
                &resource.id(),
                ReplaceResourceContent::new(bytes.len() as u64, checksum, resource.revision()),
                Box::pin(futures_util::stream::iter(
                    bytes
                        .chunks(1)
                        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                        .collect::<Vec<_>>(),
                )),
            )
            .await;
        assert!(matches!(result, Err(CoreError::InvalidOperation { .. })));
        assert_eq!(
            infrastructure
                .resource_store()
                .load(&resource.id())
                .await
                .unwrap(),
            Some(resource.clone())
        );
        assert_eq!(std::fs::read(root.join("note.txt")).unwrap(), b"old");
        assert!(
            infrastructure
                .content_replacement_store()
                .list_pending()
                .await
                .unwrap()
                .is_empty()
        );
        assert!(!root.join(".asset-hub/uploads").exists());
    }
    let bytes = "A中🙂éZ".as_bytes();
    let checksum = Checksum::sha256(
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
    .unwrap();
    let updated = runtime
        .content_service()
        .replace(
            &resource.id(),
            ReplaceResourceContent::new(bytes.len() as u64, checksum, resource.revision()),
            Box::pin(futures_util::stream::iter(
                bytes
                    .chunks(1)
                    .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                    .collect::<Vec<_>>(),
            )),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.revision(), resource.revision() + 1);
    assert_eq!(std::fs::read(root.join("note.txt")).unwrap(), bytes);
    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn fresh_resource_rename_cannot_adopt_an_unrelated_destination_blob() {
    let (root, runtime, infrastructure) = recovery_environment("rename-missing-source").await;
    let resource = Resource::builder("missing.txt")
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    std::fs::write(root.join("occupied.txt"), b"unrelated").unwrap();
    let result = runtime
        .resource_service()
        .update(
            &resource.id(),
            asset_core::resource::service::UpdateResource::new(resource.revision())
                .with_name("occupied.txt"),
        )
        .await;
    assert!(matches!(
        result,
        Err(asset_core::CoreError::Conflict { .. })
    ));
    assert_eq!(
        infrastructure
            .resource_store()
            .load(&resource.id())
            .await
            .unwrap(),
        Some(resource)
    );
    assert!(
        infrastructure
            .resource_relocation_store()
            .load_all()
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        std::fs::read(root.join("occupied.txt")).unwrap(),
        b"unrelated"
    );
    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn root_directory_empty_update_obeys_revision_without_requiring_a_parent() {
    let (root, runtime, infrastructure) = recovery_environment("root-empty-update").await;
    let directories = runtime.directory_service();
    let original = directories
        .find_by_id(&Directory::root().id())
        .await
        .unwrap();
    let revision = original.directory().revision();
    let unchanged = directories
        .update(
            &original.id(),
            asset_core::directory::service::UpdateDirectory::new(revision),
        )
        .await
        .unwrap();
    assert_eq!(unchanged.directory(), original.directory());
    assert!(matches!(
        directories
            .update(
                &original.id(),
                asset_core::directory::service::UpdateDirectory::new(revision + 1)
            )
            .await,
        Err(asset_core::CoreError::RevisionConflict { .. })
    ));
    assert!(matches!(
        directories
            .update(
                &original.id(),
                asset_core::directory::service::UpdateDirectory::new(revision).with_name("renamed")
            )
            .await,
        Err(asset_core::CoreError::Conflict { .. })
    ));
    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn resource_deletion_recovers_dual_links_but_preserves_independent_files() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "asset-hub-resource-deletion-pre-commit-{}-{nonce}",
        std::process::id()
    ));
    let config = recovery_config(root.clone());
    let infrastructure = AssetInfrastructure::new(config.clone()).await.unwrap();
    let source = StorageKey::new("note.txt").unwrap();
    let staged = StorageKey::new(".asset-hub/deletions/note.txt").unwrap();
    let resource = Resource::builder("note.txt")
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    write(&root, &source, b"old");
    let deletion = ResourceDeletion::new(
        resource.id(),
        resource.revision(),
        source.clone(),
        staged.clone(),
    )
    .unwrap();
    infrastructure
        .resource_deletion_store()
        .save(&deletion)
        .await
        .unwrap();
    infrastructure
        .resource_deletion_store()
        .save(&deletion)
        .await
        .unwrap();
    std::fs::create_dir_all(root.join(".asset-hub/deletions")).unwrap();
    // Equal contents alone are insufficient proof that a move was interrupted.
    write(&root, &staged, b"old");
    assert!(matches!(
        AssetRuntime::new(config.clone()).await,
        Err(CoreError::Conflict { .. })
    ));
    assert_eq!(std::fs::read(root.join(source.as_str())).unwrap(), b"old");
    assert_eq!(std::fs::read(root.join(staged.as_str())).unwrap(), b"old");
    assert_eq!(
        infrastructure
            .resource_deletion_store()
            .list_pending()
            .await
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_file(root.join(staged.as_str())).unwrap();
    std::fs::hard_link(root.join(source.as_str()), root.join(staged.as_str())).unwrap();
    drop(infrastructure);

    let runtime = AssetRuntime::new(config).await.unwrap();
    assert!(
        runtime
            .resource_service()
            .get(&resource.id())
            .await
            .unwrap()
            .is_none()
    );
    assert!(!root.join(source.as_str()).exists());
    assert!(!root.join(staged.as_str()).exists());
    assert_eq!(
        runtime
            .resource_recovery_service()
            .recover_pending_deletions()
            .await
            .unwrap(),
        0
    );

    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn resource_deletion_recovers_after_database_commit_without_removing_new_visible_content() {
    let (root, runtime, infrastructure) =
        recovery_environment("resource-deletion-post-commit").await;
    let source = StorageKey::new("note.txt").unwrap();
    let staged = StorageKey::new(".asset-hub/deletions/note.txt").unwrap();
    let resource = Resource::builder("note.txt")
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    let deletion = ResourceDeletion::new(
        resource.id(),
        resource.revision(),
        source.clone(),
        staged.clone(),
    )
    .unwrap();
    infrastructure
        .resource_deletion_store()
        .save(&deletion)
        .await
        .unwrap();
    write(&root, &staged, b"old");
    write(&root, &source, b"new");
    assert!(
        infrastructure
            .resource_store()
            .delete_if_revision(&resource.id(), resource.revision())
            .await
            .unwrap()
    );

    runtime
        .resource_recovery_service()
        .recover_pending_deletions()
        .await
        .unwrap();
    assert_eq!(std::fs::read(root.join(source.as_str())).unwrap(), b"new");
    assert!(!root.join(staged.as_str()).exists());
    assert!(
        infrastructure
            .resource_deletion_store()
            .list_pending()
            .await
            .unwrap()
            .is_empty()
    );

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn resource_deletion_conflict_restores_to_the_newer_resource_path() {
    let (root, runtime, infrastructure) = recovery_environment("resource-deletion-conflict").await;
    let source = StorageKey::new("note.txt").unwrap();
    let staged = StorageKey::new(".asset-hub/deletions/note.txt").unwrap();
    let target = StorageKey::new("renamed.txt").unwrap();
    let resource = Resource::builder("note.txt")
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    let deletion = ResourceDeletion::new(
        resource.id(),
        resource.revision(),
        source.clone(),
        staged.clone(),
    )
    .unwrap();
    infrastructure
        .resource_deletion_store()
        .save(&deletion)
        .await
        .unwrap();
    write(&root, &staged, b"old");
    let mut concurrent = resource.clone();
    concurrent.rename("renamed.txt").unwrap();
    assert!(
        infrastructure
            .resource_store()
            .update_if_revision(&concurrent, resource.revision())
            .await
            .unwrap()
    );

    assert!(matches!(
        runtime
            .resource_recovery_service()
            .recover_pending_deletions()
            .await,
        Err(asset_core::CoreError::RevisionConflict { .. })
    ));
    let current = infrastructure
        .resource_store()
        .load(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.name(), "renamed.txt");
    assert_eq!(std::fs::read(root.join(target.as_str())).unwrap(), b"old");
    assert!(!root.join(source.as_str()).exists());
    assert!(!root.join(staged.as_str()).exists());
    assert!(
        infrastructure
            .resource_deletion_store()
            .list_pending()
            .await
            .unwrap()
            .is_empty()
    );

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn upload_resumes_and_recovers_after_restart() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "asset-hub-direct-upload-{}-{nonce}",
        std::process::id()
    ));
    let config = recovery_config(root.clone());
    let runtime = AssetRuntime::new(config.clone()).await.unwrap();
    let directory = runtime
        .directory_service()
        .create(&Directory::root().id(), "uploads")
        .await
        .unwrap();
    let checksum =
        Checksum::sha256("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
            .unwrap();
    let command = asset_core::resource::service::CreateUpload::new(
        "note.txt",
        directory.id(),
        5,
        checksum.clone(),
    )
    .with_idempotency_key(IdempotencyKey::new("direct-upload-note").unwrap());
    let uploads = runtime.upload_service();
    let session = uploads.create(command.clone()).await.unwrap();
    let replay = uploads.create(command).await.unwrap();
    assert_eq!(replay.id(), session.id());
    assert_eq!(
        uploads
            .append(
                &session.id(),
                0,
                Checksum::sha256(
                    "372f7e2fd2d01ce2a1d71dc072acbba4c6fd25a1087cd7f153f4ec0ce37e1ede",
                )
                .unwrap(),
                upload_stream(b"he"),
            )
            .await
            .unwrap()
            .offset(),
        2
    );

    drop(runtime);

    let runtime = AssetRuntime::new(config.clone()).await.unwrap();
    let uploads = runtime.upload_service();
    assert_eq!(uploads.status(&session.id()).await.unwrap().offset(), 2);
    uploads
        .append(
            &session.id(),
            2,
            Checksum::sha256("13d896353557f29e6c8aac4bde65c743f4206df820ff8328ae567f924189d339")
                .unwrap(),
            upload_stream(b"llo"),
        )
        .await
        .unwrap();
    uploads.request_finalization(&session.id()).await.unwrap();

    drop(runtime);

    let runtime = AssetRuntime::new(config).await.unwrap();
    let uploads = runtime.upload_service();
    let mut completed = None;
    for _ in 0..100 {
        let status = uploads.status(&session.id()).await.unwrap();
        if status.status() == UploadStatus::Completed {
            completed = Some(status);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let completed = completed.expect("upload finalization was not recovered after restart");
    assert!(
        runtime
            .resource_service()
            .get(&completed.resource_id())
            .await
            .unwrap()
            .is_some()
    );

    let cancelled = uploads
        .create(asset_core::resource::service::CreateUpload::new(
            "cancelled.txt",
            directory.id(),
            0,
            Checksum::sha256("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                .unwrap(),
        ))
        .await
        .unwrap();
    uploads.abort(&cancelled.id()).await.unwrap();
    assert!(uploads.status(&cancelled.id()).await.is_err());

    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn content_replacement_recovers_after_post_publish_crash_and_is_idempotent() {
    let (root, runtime, infrastructure) = recovery_environment("content-recovery-publish").await;
    let target = StorageKey::new("note.txt").unwrap();
    let staged = StorageKey::new(".asset-hub/uploads/replacement-publish").unwrap();
    let backup = StorageKey::new(".asset-hub/content-backups/replacement-publish").unwrap();
    let old_content = verified_content(3);
    let new_content = verified_content(4);
    let resource = Resource::builder("note.txt")
        .with_content(old_content.clone())
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    let replacement = ResourceContentReplacement::new(
        resource.id(),
        resource.revision(),
        target.clone(),
        staged.clone(),
        backup.clone(),
        new_content,
    )
    .unwrap();
    infrastructure
        .content_replacement_store()
        .save(&replacement)
        .await
        .unwrap();

    // The original Blob has been moved aside and the staged replacement published, but the
    // Resource CAS never ran.
    write(&root, &target, b"new!");
    write(&root, &backup, b"old");

    assert_eq!(
        runtime
            .content_recovery_service()
            .resume_pending_replacements()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        runtime
            .content_recovery_service()
            .resume_pending_replacements()
            .await
            .unwrap(),
        0
    );
    let recovered = infrastructure
        .resource_store()
        .load(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.revision(), resource.revision());
    assert_eq!(recovered.content(), Some(&old_content));
    assert_eq!(
        read_all(infrastructure.content_reader().as_ref(), &target).await,
        Some(Bytes::from_static(b"old"))
    );
    assert!(
        read_all(infrastructure.content_reader().as_ref(), &backup)
            .await
            .is_none()
    );
    assert!(
        read_all(infrastructure.content_reader().as_ref(), &staged)
            .await
            .is_none()
    );

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn content_replacement_recovers_after_intent_before_filesystem_change() {
    let (root, runtime, infrastructure) = recovery_environment("content-recovery-intent").await;
    let target = StorageKey::new("note.txt").unwrap();
    let staged = StorageKey::new(".asset-hub/uploads/replacement-intent").unwrap();
    let backup = StorageKey::new(".asset-hub/content-backups/replacement-intent").unwrap();
    let old_content = verified_content(3);
    let resource = Resource::builder("note.txt")
        .with_content(old_content.clone())
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    infrastructure
        .content_replacement_store()
        .save(
            &ResourceContentReplacement::new(
                resource.id(),
                resource.revision(),
                target.clone(),
                staged.clone(),
                backup.clone(),
                verified_content(4),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    write(&root, &target, b"old");
    write(&root, &staged, b"new!");

    assert_eq!(
        runtime
            .content_recovery_service()
            .resume_pending_replacements()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        read_all(infrastructure.content_reader().as_ref(), &target).await,
        Some(Bytes::from_static(b"old"))
    );
    assert!(
        read_all(infrastructure.content_reader().as_ref(), &staged)
            .await
            .is_none()
    );
    assert!(
        read_all(infrastructure.content_reader().as_ref(), &backup)
            .await
            .is_none()
    );

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn directory_recovery_is_idempotent_after_filesystem_move() {
    let (root, runtime, infrastructure) =
        recovery_environment("directory-relocation-recovery").await;
    let directories = runtime.directory_service();
    let source = directories
        .create(&Directory::root().id(), "source")
        .await
        .unwrap();
    let child = directories.create(&source.id(), "child").await.unwrap();
    let resource = Resource::builder("note.txt")
        .with_directory_id(child.id())
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();

    let current = directories.find_by_id(&source.id()).await.unwrap();
    let expected_revision = current.directory().revision();
    let mut desired = current.directory().clone();
    desired.rename("destination").unwrap();
    let destination = DirectoryPath::from_path("destination").unwrap();
    let relocation = asset_core::directory::port::DirectoryRelocation::new(
        source.id(),
        source.path().clone(),
        destination.clone(),
        vec![DirectoryRevisionUpdate::new(desired, expected_revision).unwrap()],
    )
    .unwrap();
    infrastructure
        .directory_relocation_store()
        .begin(&relocation)
        .await
        .unwrap();

    std::fs::rename(root.join("source"), root.join("destination")).unwrap();
    assert_eq!(
        runtime
            .directory_recovery_service()
            .recover_pending_relocations()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        runtime
            .directory_recovery_service()
            .recover_pending_relocations()
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        directories.find_by_id(&source.id()).await.unwrap().path(),
        &destination
    );
    assert_eq!(
        directories
            .find_by_id(&child.id())
            .await
            .unwrap()
            .path()
            .path(),
        "destination/child"
    );
    assert_eq!(
        infrastructure
            .resource_store()
            .load(&resource.id())
            .await
            .unwrap()
            .unwrap()
            .directory_id(),
        child.id()
    );
    assert!(!root.join("source").exists());
    assert!(root.join("destination/child").is_dir());

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn directory_recovery_moves_source_when_only_the_intent_was_persisted() {
    let (root, runtime, infrastructure) = recovery_environment("directory-relocation-intent").await;
    let directories = runtime.directory_service();
    let source = directories
        .create(&Directory::root().id(), "source")
        .await
        .unwrap();
    let current = directories.find_by_id(&source.id()).await.unwrap();
    let expected_revision = current.directory().revision();
    let mut desired = current.directory().clone();
    desired.rename("destination").unwrap();
    let destination = DirectoryPath::from_path("destination").unwrap();
    infrastructure
        .directory_relocation_store()
        .begin(
            &asset_core::directory::port::DirectoryRelocation::new(
                source.id(),
                source.path().clone(),
                destination.clone(),
                vec![DirectoryRevisionUpdate::new(desired, expected_revision).unwrap()],
            )
            .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        runtime
            .directory_recovery_service()
            .recover_pending_relocations()
            .await
            .unwrap(),
        1
    );
    assert!(!root.join("source").exists());
    assert!(root.join("destination").is_dir());
    assert_eq!(
        directories.find_by_id(&source.id()).await.unwrap().path(),
        &destination
    );

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn directory_recovery_keeps_intent_when_source_and_destination_both_exist() {
    let (root, runtime, infrastructure) =
        recovery_environment("directory-relocation-ambiguous").await;
    let directories = runtime.directory_service();
    let source = directories
        .create(&Directory::root().id(), "source")
        .await
        .unwrap();
    let current = directories.find_by_id(&source.id()).await.unwrap();
    let expected_revision = current.directory().revision();
    let mut desired = current.directory().clone();
    desired.rename("destination").unwrap();
    let destination = DirectoryPath::from_path("destination").unwrap();
    infrastructure
        .directory_relocation_store()
        .begin(
            &asset_core::directory::port::DirectoryRelocation::new(
                source.id(),
                source.path().clone(),
                destination,
                vec![DirectoryRevisionUpdate::new(desired, expected_revision).unwrap()],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    std::fs::create_dir(root.join("destination")).unwrap();

    assert!(matches!(
        runtime
            .directory_recovery_service()
            .recover_pending_relocations()
            .await,
        Err(asset_core::CoreError::Conflict { .. })
    ));
    assert!(
        infrastructure
            .directory_relocation_store()
            .load_pending()
            .await
            .unwrap()
            .iter()
            .any(|pending| pending.directory_id() == source.id())
    );
    assert!(root.join("source").is_dir());
    assert!(root.join("destination").is_dir());

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn resource_relocation_recovers_interrupted_hard_link_move() {
    let (root, runtime, infrastructure) =
        recovery_environment("resource-relocation-recovery").await;
    let directories = runtime.directory_service();
    let source = directories
        .create(&Directory::root().id(), "source")
        .await
        .unwrap();
    let destination = directories
        .create(&Directory::root().id(), "destination")
        .await
        .unwrap();
    let resource = Resource::builder("note.txt")
        .with_directory_id(source.id())
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    let source_key = StorageKey::new("source/note.txt").unwrap();
    let destination_key = StorageKey::new("destination/note.txt").unwrap();
    write(&root, &source_key, b"old");

    let mut desired = resource.clone();
    desired.move_to_directory(destination.id()).unwrap();
    let relocation = ResourceRelocation::new(
        desired.clone(),
        resource.revision(),
        source_key.clone(),
        destination_key.clone(),
    )
    .unwrap();
    infrastructure
        .resource_relocation_store()
        .save(&relocation)
        .await
        .unwrap();
    std::fs::hard_link(
        root.join(source_key.as_str()),
        root.join(destination_key.as_str()),
    )
    .unwrap();

    assert_eq!(
        runtime
            .resource_recovery_service()
            .recover_pending_relocations()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        runtime
            .resource_recovery_service()
            .recover_pending_relocations()
            .await
            .unwrap(),
        0
    );
    let recovered = infrastructure
        .resource_store()
        .load(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.directory_id(), destination.id());
    assert_eq!(recovered.revision(), desired.revision());
    assert!(!root.join(source_key.as_str()).exists());
    assert!(root.join(destination_key.as_str()).is_file());

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn resource_relocation_rolls_back_physical_move_when_a_newer_revision_wins() {
    let (root, runtime, infrastructure) = recovery_environment("resource-relocation-stale").await;
    let directories = runtime.directory_service();
    let source = directories
        .create(&Directory::root().id(), "source")
        .await
        .unwrap();
    let destination = directories
        .create(&Directory::root().id(), "destination")
        .await
        .unwrap();
    let resource = Resource::builder("note.txt")
        .with_directory_id(source.id())
        .with_content(verified_content(3))
        .build()
        .unwrap();
    infrastructure
        .resource_store()
        .insert(&resource)
        .await
        .unwrap();
    let source_key = StorageKey::new("source/note.txt").unwrap();
    let destination_key = StorageKey::new("destination/note.txt").unwrap();
    write(&root, &source_key, b"old");

    let mut desired = resource.clone();
    desired.move_to_directory(destination.id()).unwrap();
    infrastructure
        .resource_relocation_store()
        .save(
            &ResourceRelocation::new(
                desired,
                resource.revision(),
                source_key.clone(),
                destination_key.clone(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    std::fs::rename(
        root.join(source_key.as_str()),
        root.join(destination_key.as_str()),
    )
    .unwrap();

    let mut concurrent = resource.clone();
    concurrent.rename("concurrent.txt").unwrap();
    assert!(
        infrastructure
            .resource_store()
            .update_if_revision(&concurrent, resource.revision())
            .await
            .unwrap()
    );

    assert!(matches!(
        runtime
            .resource_recovery_service()
            .recover_pending_relocations()
            .await,
        Err(asset_core::CoreError::RevisionConflict { .. })
    ));
    let current = infrastructure
        .resource_store()
        .load(&resource.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.name(), concurrent.name());
    assert_eq!(current.revision(), concurrent.revision());
    assert!(root.join(source_key.as_str()).is_file());
    assert!(!root.join(destination_key.as_str()).exists());
    assert!(
        infrastructure
            .resource_relocation_store()
            .load_all()
            .await
            .unwrap()
            .is_empty()
    );

    drop(infrastructure);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

async fn wait_for_resource(
    runtime: &AssetRuntime,
    directory: &DirectoryPath,
    name: &str,
) -> Option<Resource> {
    for _ in 0..100 {
        if let Some(resource) = find_resource(runtime, directory, name).await {
            return Some(resource);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

async fn wait_until_absent(runtime: &AssetRuntime, directory: &DirectoryPath, name: &str) {
    for _ in 0..100 {
        if find_resource(runtime, directory, name).await.is_none() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("resource `{directory}/{name}` was not removed by automatic synchronization");
}

async fn find_resource(
    runtime: &AssetRuntime,
    directory: &DirectoryPath,
    name: &str,
) -> Option<Resource> {
    let directory = runtime
        .directory_service()
        .find_by_path(directory)
        .await
        .ok()?;
    let page = runtime
        .resource_service()
        .list(ListResources::new(100, 0, directory.id()))
        .await
        .ok()?;
    page.items
        .into_iter()
        .find(|located| located.resource().name() == name)
        .map(|located| located.into_resource())
}

async fn root_directory_paths(runtime: &AssetRuntime) -> Vec<DirectoryPath> {
    let directories = runtime.directory_service();
    let root = directories.root().await.unwrap();
    directories
        .list_children(&root.id())
        .await
        .unwrap()
        .into_iter()
        .map(|directory| directory.path().clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn local_storage_changes_are_synchronized_automatically() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "asset-hub-auto-sync-{}-{nonce}",
        std::process::id()
    ));
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite: SqliteDatabaseConfig { max_connections: 1 },
            ..DatabaseConfig::default()
        },
        blob: BlobConfig {
            local: LocalBlobConfig {
                root: root.clone(),
                sync: LocalBlobSyncConfig {
                    enabled: true,
                    debounce_milliseconds: 50,
                    reconcile_interval_seconds: 1,
                },
            },
            ..BlobConfig::default()
        },
        ..AssetInfraConfig::default()
    };
    let mut runtime = AssetRuntime::new(config).await.unwrap();
    runtime.start_storage_sync().await.unwrap();
    let directory = DirectoryPath::from_path("documents").unwrap();
    let directory_path = root.join("documents");
    std::fs::create_dir_all(&directory_path).unwrap();
    std::fs::write(directory_path.join("note.txt"), b"first").unwrap();

    let first = wait_for_resource(&runtime, &directory, "note.txt")
        .await
        .expect("new file should be imported automatically");
    std::fs::write(directory_path.join("note.txt"), b"second version").unwrap();
    let mut updated = None;
    for _ in 0..100 {
        let resource = find_resource(&runtime, &directory, "note.txt")
            .await
            .unwrap();
        if resource.content().unwrap().size() == 14 {
            updated = Some(resource);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let updated = updated.expect("modified file should update Resource content");
    assert_eq!(updated.id(), first.id());

    std::fs::rename(
        directory_path.join("note.txt"),
        directory_path.join("renamed.txt"),
    )
    .unwrap();
    let renamed = wait_for_resource(&runtime, &directory, "renamed.txt")
        .await
        .expect("renamed file should be synchronized automatically");
    assert_eq!(renamed.id(), first.id());

    std::fs::remove_file(directory_path.join("renamed.txt")).unwrap();
    wait_until_absent(&runtime, &directory, "renamed.txt").await;

    let empty_directory = root.join("empty");
    std::fs::create_dir(&empty_directory).unwrap();
    let mut directory_created = false;
    for _ in 0..100 {
        if root_directory_paths(&runtime)
            .await
            .iter()
            .any(|path| path.path() == "empty")
        {
            directory_created = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(directory_created, "new directory should be synchronized");
    std::fs::remove_dir(&empty_directory).unwrap();
    let mut directory_removed = false;
    for _ in 0..100 {
        if root_directory_paths(&runtime)
            .await
            .iter()
            .all(|path| path.path() != "empty")
        {
            directory_removed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        directory_removed,
        "removed directory should be synchronized"
    );

    let managed = runtime
        .directory_service()
        .create(&Directory::root().id(), "managed-empty")
        .await
        .unwrap();
    let managed_path = managed.path().clone();
    assert!(root.join(managed_path.path()).is_dir());
    let directories = runtime.directory_service();
    let managed = directories.find_by_id(&managed.id()).await.unwrap();
    assert!(
        directories
            .delete(&managed.id(), managed.directory().revision())
            .await
            .unwrap()
    );
    assert!(!root.join(managed_path.path()).exists());
    runtime
        .storage_maintenance_service()
        .reconcile_storage()
        .await
        .unwrap();
    assert!(
        runtime
            .directory_service()
            .find_by_path(&managed_path)
            .await
            .is_err(),
        "service deletion must not be re-imported by reconciliation"
    );

    drop(runtime);
    tokio::task::yield_now().await;
    let _ = std::fs::remove_dir_all(&root);
}
